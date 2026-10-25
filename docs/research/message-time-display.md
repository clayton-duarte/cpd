# Render-time timestamps for the pi chat: the User prompt balloon and tool rows

Plan `message-time`, Job `msggaps-docs` (Attempt 1). Research only: no code changed.
Read on 2026-10-24 from the Node install at
`~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent`
(`dist/`-relative paths below) and the cpd worktree (repo-relative paths). Everything
is from reading source; **no live pi pane was driven**, so every runtime claim is a
claim unless the same file also proves it. Facts I only inferred are labelled
"infer:".

## 0. Verdict

| Surface | Feasible with cpd-only changes? | The reachable value | Recommendation |
|---|---|---|---|
| cpd fold | **yes, today** | `CustomMessage.timestamp` (ms) passed to the registered message renderer | implement now |
| agent balloon + thinking rows | **yes, via a private field cpd already reads** | `AssistantMessage.timestamp` (ms) on `AssistantMessageComponent.lastMessage` | implement now, with a guard |
| User prompt balloon | **no public route**; only fragile correlation or content mutation | none exposed to any renderer | defer (or take it upstream) |
| tool execution rows | **yes, two cpd-owned sources** | `ctx.state.startedAt`/`endedAt` live; the session entry's `timestamp` on resume | implement now (state) + resolve the entry on resume |

"cpd-only" means the cpd repo, including its own `pi/compact-tools` fork
(`pi/package.json` lists `./compact-tools/extensions/compact-tools.ts` as a loaded
extension): no upstream pi change.

## 1. cpd fold: `CustomMessage.timestamp` -- the assumption holds

The message renderer pi hands a cpd custom message already carries the time.

- pi's `MessageRenderer` is `(message: CustomMessage<T>, options, theme) => Component`
  (`dist/core/extensions/types.d.ts:1169`), and `CustomMessage` has
  `timestamp: number` (`dist/core/messages.d.ts:32-39`, field at `:38`).
- The value is milliseconds built from the persisted entry:
  `sessionEntryToContextMessages` re-creates it as
  `createCustomMessage(..., entry.timestamp)` (`dist/core/session-manager.js:179-182`),
  and `createCustomMessage` does `timestamp: new Date(timestamp).getTime()`
  (`dist/core/messages.js:57-66`, line `:64`). `appendCustomMessageEntry` writes the
  entry as `timestamp: new Date().toISOString()` (`dist/core/session-manager.js:947-960`,
  line `:956`). Entry timestamps are ISO 8601 strings, message timestamps are Unix
  milliseconds (`docs/session-format.md:45-47`, `docs/message-types.md:5`).
- pi passes that exact object to the renderer from `CustomMessageComponent`
  (`dist/modes/interactive/components/custom-message.d.ts:16`,
  `dist/modes/interactive/interactive-mode.js:3164-3169`).

So the fold's time is stable across start, `/reload` and resume, because it is stored,
not recomputed. **Correction to the cpd side of the assumption:** cpd does not read it
yet. cpd's own narrowed type `RenderedMessage`
(`pi/lib/pi.ts:31-32`) is only `{ customType; content; details? }`; it drops
`timestamp`. `pi/lib/render.ts` (`rowsOf`, `Rendering.render`, `:160-230`) draws no
time. Implementing is a one-line type widening plus a row; tests already build the
full shape with `timestamp: 0` (`pi/test/pill-split.test.ts:18`,
`pi/test/folds.test.ts:20`). The assumption is therefore **correct about pi and
currently unused by cpd**.

## 2. Agent balloon and thinking rows: `AssistantMessage.timestamp` -- holds, via a private field

- `AssistantMessage.timestamp` is "Unix timestamp in milliseconds when the request
  started" (`@earendil-works/pi-ai/dist/types.d.ts:362-393`, field at `:385-386`).
- `AssistantMessageComponent` stores the message it renders in a private `lastMessage`
  (`dist/modes/interactive/components/assistant-message.d.ts:7-25`, field at `:14`);
  `updateContent(message)` assigns it (`dist/.../assistant-message.js:69-71`,
  line `:70`). The constructor and `updateContent` receive pi's own persisted message
  (`dist/.../assistant-message.js:21-34`, `interactive-mode.js:3218-3222`), so on
  resume the timestamp is the stored one, not `Date.now()`.
- There is no public getter, but **cpd already reads this private field**:
  `AssistantLike = { lastMessage?: ... }` and the group renderer reads
  `(child as AssistantLike).lastMessage?.content`
  (`pi/compact-tools/extensions/compact-tools-grouping.ts:250,254-255`). cpd also
  already wraps `AssistantMessageComponent.updateContent` once per process
  (`pi/compact-tools/extensions/compact-tools-thinking.ts:304-311`). So reaching
  `lastMessage.timestamp` needs no new upstream surface.
- Thinking rows are drawn from the same message (`assistant-message.js:80-132`), so
  the only timestamp available for them is the assistant request start; there is no
  per-thinking-block time (`ThinkingContent` has no timestamp,
  `@earendil-works/pi-ai/dist/types.d.ts:251-259`). "Thinking rows show the assistant
  turn's time" is the honest statement, not a per-row time.

Assumption verdict: **correct**, with the caveat that it is private-member access, not
a documented API; the code already depends on that private member elsewhere.

## 3. User prompt balloon: no public route

`UserMessageComponent` is constructed with text only and stores no time:

- Constructor `(text, markdownTheme?, outputPad?, markdownTransformers?)`
  (`dist/modes/interactive/components/user-message.d.ts:6-15`); the runtime fields are
  `text`, `markdownTheme`, `outputPad`, `markdownTransformers`
  (`dist/.../user-message.js:10-22`), and `rebuild()` only wraps the text in a
  `Markdown` (`:27-39`). No `timestamp`, no entry id.
- pi builds it from `this.getUserMessageText(message)`, which keeps only text blocks
  and drops `message.timestamp` (`interactive-mode.js:3088-3095`, construction at
  `:3189-3216`, lines `:3204` and `:3209`).

No extension hook exposes the user message or its entry to a renderer:

- `registerMessageRenderer` is only for `CustomMessage` custom types
  (`dist/core/extensions/types.d.ts:1250`); there is no user-message renderer.
- `registerMarkdownTransformer` is the only hook on user text
  (`types.d.ts:1252`), and its context is
  `{ messageType, isStreaming, availableWidth }` (`types.d.ts:1160-1165`), applied by
  `createMarkdownTransform("user", false, ...)` (`user-message.js:37`,
  `dist/.../markdown-transform.js:1-3`). It has no timestamp and no entry id, and is
  called on every render, so it cannot by itself know *which* user message it is
  editing.

Two cpd-only routes exist, both rejected:

1. **Patch `UserMessageComponent.prototype.render`** (cpd already does exactly this
   for its activity row: `pi/compact-tools/extensions/compact-tools-activity.ts:191-214`)
   and correlate `self.text` to an entry. The session does expose the entries and
   `buildContextEntries()` (`dist/core/session-manager.d.ts:178,311,316,334`;
   `ExtensionContext.sessionManager`, `types.d.ts:223`), and pi's transcript is built
   from `sessionManager.buildContextEntries()` (`interactive-mode.js:3460`). But the
   component carries no id, so the map is text (duplicate prompts collide) or
   position (incremental transcript paths rebuild subsets at
   `interactive-mode.js:2802,2811,3011`, so position does not line up). This is a
   fragile guess, not a mapping.
2. **Mutate the user's text** from the `input` event's `transform` result
   (`dist/core/extensions/types.d.ts:922-927`) to embed a time. That changes what the
   model sees and what is persisted, contradicts the adapter's display-only contract
   (`pi/lib/render.ts:14-16`), and is out of scope.

Recommendation: **defer.** If this must be shown, request a user-message timestamp
(or a user renderer) upstream rather than correlating.

## 4. Tool execution rows: feasible from two cpd-owned sources

pi's tool render context deliberately has no time:

- `ToolRenderContext` (`dist/core/extensions/types.d.ts:347-379`) carries
  `toolCallId` (`:351`), `executionStarted` (`:361`) and `durationMs` (`:376`), but no
  timestamp and no `sessionManager`/entry id. `ToolExecutionStartEvent` also has no
  timestamp (`types.d.ts:853-859`), only `toolCallId`.

But cpd owns the renderers and already times every row:

- **Live rows.** cpd's fork registers the row renderers and records a wall clock.
  `pi.on("tool_execution_start", ...)` calls `runtime.noteExecutionStart(id)`
  (`pi/compact-tools/extensions/compact-tools.ts:890-897`), which stores
  `startedAt: Date.now()` (`compact-tools-runtime.ts:200-203`); the end event stores
  `endedAt` and the host `durationMs` (`:205-211`). Both are copied into the renderer's
  `RowState` by `syncRow` (`compact-tools-runtime.ts:180-197`, lines `:191-193`) and
  `restoreTiming` (`:263-269`). `RowState.startedAt` / `endedAt` are part of the type
  the renderers receive (`compact-tools-types.ts:44,54-55`). The call and result
  renderers call `syncRow` before drawing (`compact-tools.ts:301,511`), so
  `ctx.state.startedAt` / `ctx.state.endedAt` are already there for
  `compact`/`claude`/`codex`. This is a cpd-only source that needs no session lookup.
- **Resumed / reloaded rows.** The in-memory timings are lost: a resumed row has
  `executionStarted === false`, `restoreTiming` misses, and the code's own comment
  says a restored row "has no time to measure unless this process timed it before a
  /reload" (`compact-tools-runtime.ts:182-189`). For those rows, resolve the persisted
  entry by `ctx.toolCallId`:
  - tool result entry: `entry.type === "message" && entry.message.role === "toolResult"`
    with `entry.message.toolCallId === ctx.toolCallId`; `ToolResultMessage` has both
    `toolCallId` (`:416`) and `timestamp` ms (`:426`) in
    `@earendil-works/pi-ai/dist/types.d.ts:414-429`;
  - or the assistant entry holding the call (`ToolCall.id`, same file `:265-278`,
    field `:267`; `AssistantMessage.timestamp` `:385-386`); pi creates the row
    component from `content.id` (`interactive-mode.js:3262-3270`) and matches the
    result by `message.toolCallId` (`:3296-3302`).
  Use `ctx.sessionManager.getBranch()` (`dist/core/session-manager.d.ts:311`, in
  `ReadonlySessionManager` `:178`) rather than `getEntries()`, so an abandoned branch
  cannot supply the time. `ExtensionContext.sessionManager` is already narrowed into
  cpd's `Ctx` (`pi/lib/pi.ts:5-8`) and cpd holds it live across the session
  (`pi/lib/host.ts:42-43`, set at `pi/lib/startup.ts:39`, cleared at `:256`).
  Reaching it from a renderer needs the resolver closure; cpd's `PiApi` does not
  include `registerToolRenderer` today (`pi/lib/pi.ts:16-19`), so that one line must
  be added, or the time must be drawn inside the existing compact-tools renderers
  (`compact-tools.ts:605-607,647-648`).

Recommendation: **implement** — draw `ctx.state.startedAt`/`endedAt` while the row is
live, and fall back to the resolved branch entry's timestamp when the row is restored.
Prefer the result entry's time for "when it ran"; note the assistant entry's time is
the request start, not the tool's start.

## 5. Failure modes, per approach

| Failure | cpd fold (`message.timestamp`) | agent / thinking (`lastMessage.timestamp`) | tool rows (state + entry) |
|---|---|---|---|
| `/reload` | none: the renderer is re-registered each load and the value is read from the passed message (loader `dist/core/extensions/loader.js:278`; renderer resolved first-match in `dist/core/extensions/runner.js:529-537`). | wrapper replaced, not stacked: `hookMethod` keeps one wrapper and swaps behaviour by symbol (`compact-tools-hook.ts:20-47`). | in-memory `executionTimings` are gone; the live state path yields nothing (`compact-tools-runtime.ts:182-189`), so the entry fallback is required. `host.ctx` is refreshed on the new `session_start` (`startup.ts:39`). |
| resumed session / rebuilt old rows | correct: the entry is re-created from the stored ISO time (`session-manager.js:179-182`). | correct: the constructor/`updateContent` get the persisted message (`assistant-message.js:21-34`). | live state absent; resolve from `getBranch()`. Never `getEntries()`, which includes abandoned branches (pi's own docs say to rebuild from `getBranch()`, `docs/extensions.md:236`). |
| compaction | unaffected; hidden custom messages are simply not rendered. | unaffected. | a summarized-away call/result has no entry left; if `firstKeptEntryId` splits a call from its result, only the call entry can be found — fall back to it, or draw no time. |
| streaming | user/cpd messages are not streamed. | `lastMessage` is updated each frame, so the time is present but is the request start from the first frame (`assistant-message.js:69-71`). | `renderResult` runs with `isPartial` while the entry is not persisted yet (pi's own note: "Entries don't contain `message` yet: message_end fires before persistence", `interactive-mode.js:3402`), so use `ctx.state.startedAt`; do not assume an entry exists. |
| a second copy loaded | `getMessageRenderer` returns the first matching extension (`runner.js:529-537`), so a second copy can shadow or the renderer can be registered twice. | a second patch with a different symbol would stack; with cpd's `hookMethod` symbol it replaces (`compact-tools-hook.ts:28-32`). | resolvers run in load order and compose through `next()` (`docs/extensions.md:193`); two copies that both add a time can double-render. cpd already detects the known npm double-load for its compact-tools fork (`pi/compact-tools/extensions/compact-tools-double-load.ts:1-13,66-75`). |
| duplicate identical user prompts | n/a | n/a | text correlation (user balloon) is ambiguous; the tool-row path is keyed by unique `toolCallId`, so it is not. |

## 6. Recommendations, concrete

1. **cpd fold — implement now.** Widen `RenderedMessage` in `pi/lib/pi.ts:32` with
   `timestamp: number` and draw it from `message.timestamp` in
   `pi/lib/render.ts`. The stored value survives reload and resume.
2. **Agent balloon and thinking rows — implement now.** Read
   `AssistantMessageComponent.lastMessage.timestamp` through the same private-member
   access cpd already uses (`compact-tools-grouping.ts:250,254-255`) and the existing
   `updateContent` wrapper (`compact-tools-thinking.ts:304-311`); add a symbol guard so
   a reload or a second copy cannot draw it twice. State in the code that it is a
   private field and the timestamp is the assistant request start.
3. **User prompt balloon — defer.** No public hook carries a user-message time
   (`user-message.d.ts:6-15`, `types.d.ts:1160-1165,1250`). The only cpd-only routes
   are correlated text/order matching (fragile) or mutating the user's content
   (changes LLM context). Ask upstream for a user-message timestamp rather than
   shipping either.
4. **Tool execution rows — implement now.** Draw `ctx.state.startedAt`/`endedAt`
   (`compact-tools-types.ts:54-55`, populated by `compact-tools-runtime.ts:191-195`)
   for live rows, and resolve the restored row's time from
   `host.ctx?.sessionManager.getBranch()` by `ctx.toolCallId` (`session-manager.d.ts:311`,
   `pi-ai/dist/types.d.ts:416,426`). Requires adding `registerToolRenderer` to
   `PiApi` (`pi/lib/pi.ts:16-19`) or drawing inside the compact-tools renderers
   (`compact-tools.ts:605-607,647-648`).

## 7. Unverified / inferred

- Everything here is read from source; no row was observed in a running pi TUI.
- "infer:" whether pi's extension list is fully replaced on `/reload` is inferred
  from cpd's own comment that the factory is evaluated anew (`compact-tools-double-load.ts:12`)
  and from `getMessageRenderer` iterating the current list (`runner.js:529-537`); I did
  not drive a reload.
- "infer:" that a resumed `ToolExecutionComponent` always has
  `executionStarted === false`. The code comment asserts it
  (`compact-tools-runtime.ts:182-189`) and `markExecutionStarted` is only called on
  the live path (`interactive-mode.js:3262-3270` does not call it), but I did not
  resume a session to confirm.
