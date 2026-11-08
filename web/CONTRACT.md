# web/ Phase 1 contract (amended for Phase 2 by section 10)

The complete contract of the clean-slate `web/` package, Plan `webui-phase1-work` (Phase 1, "make it work", of cpd-webui). Workers write tests from this file alone. Where two statements conflict, the later section of this file does not win silently: ask the Lead (`cpd ask`). Section 10 (Phase 2, pro-d20) is the exception: where a statement of sections 1 to 9 says "superseded by 10.x", section 10 wins and the older statement is void.

Citations: `scout-cockpit.md`, `scout-durable.md`, `scout-pi.md` are the explore artefacts of this Plan (`$CPD_HOME/run/webui-phase1-work/explore/`), cited as `scout-<x> §n`. Repo paths are relative to the repo root. `node_modules/@earendil-works/pi-durable/dist/...` and `.../pi-ai/dist/types.d.ts` are the installed typings (pi-durable 1.5.0, read by reading, not run). Every unconfirmed fact is tagged `(unverified, O<n>)` and listed once in section 9 "Open items". A value this file does not cite is a decision of this contract, called a **Phase 1 choice**.

## 1. Scope

### 1.1 What this is

`web/` is a clean-slate browser UI for cpd: a 3-pane cockpit (Context tree, Plan view, Attention Inbox) around a web-native chat that attaches to a cpd Session (pi-runner or durable-host) and renders and drives it. Phase 1 is a dev bench served by `vite serve` (`pnpm --filter cpd-web-client run dev`). The legacy `web/` code is deleted (pro-d13).

### 1.2 Recorded decisions carried out

| Id | Decision (meaning) |
|---|---|
| pro-d10 | Chat scope = the features of the `cpd pi/` adapter plus (1) image and file import, (2) drag-and-drop of files onto the chat, (3) model, thinking level and Tier selection, (4) footer data (model, tier, thinking, context %, cost, git branch), (5) the chat title (session key + title, also `document.title`). |
| pro-d11 | The chat is web-native: it renders cpd's pi-adapter semantics (tool rows, folds, dialogs) as web components; it is not a terminal emulator. |
| pro-d12 | Both backends: the durable-host unix socket (`$CPD_HOME/run/host/<id>.sock`) and pi-runner sessions (session JSONL + `cpd session send` + `cpd session retier`). |
| pro-d13 | The legacy `web/` is deleted; Phase 1 starts clean. |
| pro-d14 | Phase 1 is "make it work": correctness first; no visual design. (Recorded by the Plan; no further rule is derived from it here.) |
| pro-d15 | Proof is manual in a browser, recorded in `web/VERIFY.md` (a later Job writes it; not this file). |
| pro-d16 | Minimal functional visuals only: unstyled semantic HTML plus one small plain CSS file; the Figma design comes in Phase 3. |
| pro-d17 | `web/package.json` gains exactly one dependency, `"@earendil-works/chord": "1.5.0"` (exact, no range), and `pnpm-lock.yaml` changes only for that line (pro-d377, User, 2026-11-03 EST: the freeze of this row is lifted for that one line and its lockfile change). Every other line of `web/package.json` and `pnpm-lock.yaml` is frozen: no other dependency is added or removed. The transport is Vite `server.ws` custom events, dev bench only. | **Superseded by 15.1:** the transport is the one WebSocket endpoint of §15 (ADR 0015 option B, air-d46), which adds the pinned `ws` dependency; the custom-events transport is gone. |
| pro-d18 | (User, 2026-11-03 EST, chose in this Plan: add header and status bar slots, minimal.) The cockpit is a 5-zone frame: full-width Top Header, left full-height Context tree, centre-top Plan view, centre-bottom Chat (Chat Title on its top border, composer and chat footer at its bottom), right full-height Attention Inbox, full-width Bottom Status Bar. Phase 1 adds ONLY a Top Header with the plain text `C-P-D` (no emblem art) and a Bottom Status Bar showing the selected session's runner and state and `cpd` version (or an explicit `unknown`); the Chat Title is the `ChatHeader` already built, placed at the top of the chat zone. Pill styling, emblem art, host stats, workers summary and balloons are not Phase 1 (pro-d16: Figma in Phase 3, visuals in Phase 4). No new data source and no `wire.ts` field. |
| pro-d365 | cpd consumes pi-durable as state (the `ConversationView` of host protocol 2, `view` and `ops` frames); no event-synthesis shim is built on top of it. |
| pro-d377 | (User, 2026-11-03 EST.) The web durable backend speaks host protocol 2 (view and ops, pi-durable 1.5.0) with full parity with today: live text, tool rows, busy, queue, folds, footer, paging by the host `history` command, dialogs. `web/package.json` declares `@earendil-works/chord` 1.5.0 exactly; this lifts the freeze of pro-d17 and A7 for that one line and its lockfile change, and every other freeze stays. |
| pro-d402 | (Lead, 2026-11-05 EST, decided work.) Supersedes the Stop-button removal of pro-d57 (`web/src/ui/DESIGN-compose.md`): the Composer shows a Stop button while busy when `capabilities.abort.supported` (6.9). The durable `secret_approve` and `host_approve` selects map onto `ask_user_question` (3.8); `compactionFailure` renders as a fold (6.4 live tail, L6); a durable attach retries `host_unreachable` for 15000 ms (6.4 backend). |

### 1.3 Accepted assumptions

| Id | Assumption |
|---|---|
| A1 | Browser to server transport is Vite `server.ws` custom events `cpd:c2s` (client to server) and `cpd:s2c` (server to client), through `import.meta.hot.send/on` in the browser. Dev bench only; no auth beyond Vite's localhost bind. | **Superseded by 15.1 (Plan `webui-ws`):** the browser to server transport is one WebSocket endpoint, path `/cpd-ws`, shared by the desktop and the browser build, with the Origin check of 15.1 (ADR 0015 option B, air-d46); the `cpd:c2s`/`cpd:s2c` events and the dev-only hot transport are gone. |
| A2 | Only existing dependencies (plus the one lifted line of pro-d17 and A7, `@earendil-works/chord` 1.5.0, used by the durable dialect for `applyImmutable`): react 19, react-dom, react-markdown, remark-gfm, vite, @vitejs/plugin-react, vitest, jsdom, typescript. No Tailwind, shadcn, radix, router. Unstyled semantic HTML plus one small plain CSS file (`web/src/ui/app.css`). UI tests use `react-dom/client` + `act` from `react` under `// @vitest-environment jsdom`, no testing-library. | **Superseded by 10.8 and 10.9:** Phase 2 adds no dependency (`web/package.json` and `pnpm-lock.yaml` stay byte-identical, A1 of Plan webui-phase2-fast); the durable v2 dialect applies ops with its own `applyOps` in `ops.ts`, not `applyImmutable`.
| A3 | The durable backend speaks pi-durable **1.5.0** host protocol 2 only: a `view` base frame and `ops` frames (Chord ops applied with `applyImmutable`) of the `ConversationView` state (section 6.4), with full parity in live text, tool rows, busy, queue, folds, footer, paging by the host `history` command and dialogs. The `view` and `ops` shapes were verified by a real Harness run for text and entries (`$CPD_HOME/run/pi-1-5-upgrade/explore/live-check.md`); the thinking and tool-argument append paths are unverified (O20). The code keeps ONE dialect seam (`web/src/server/durable/frames.ts`). An unknown host version or frame shape fails loud with a typed `unsupported_host_version` / `protocol_error`, never a silent fallback. | **Superseded by 10.8:** web/ supports two dialects chosen by `welcome.protocol`: v1 (protocol 1, pi-durable 1.3.0, `snapshot`/`events` frames) and v2 (protocol 2, pi-durable 1.5.0, `view`/`ops` frames). v2 is tested only against mocked frames (A2 of Plan webui-phase2-fast).
| A4 | pi-runner sessions: transcript read from the session JSONL (`file` of `cpd session list`), live by file watch, prompts through `cpd session send`, Tier through `cpd session retier`. There is NO `cpd session abort` and no TUI-state steer, so for pi sessions abort, steer and compaction are declared unsupported in typed `capabilities` with a human reason; images are supported (sent as `[Attached image: <path>]`, 11.3) and the `ask_user_question` and plan review dialogs are interactive (11.4). The UI disables the unsupported controls and shows the reason. |
| A5 | Window size is **40** transcript items for the tail and for each `loadEarlier` page (`WINDOW_SIZE`). |
| A6 | Every timer or clock is an injected `Scheduler`. No test uses real time, a real socket, a real file watcher, a real `cpd`/`git` process or the real network: all externals are injected boundaries, faked in the test. No test leaks a file, process, socket, handle or timer. |
| A7 | Only paths under `web/` are touched, plus `pnpm-lock.yaml` for the one `@earendil-works/chord` 1.5.0 line of `web/package.json` (pro-d377, the lifted line of pro-d17). `pi/durable-host/**`, `pi/durable-worker/**`, the root `package.json`, `pnpm-workspace.yaml` and every other line of `pnpm-lock.yaml` are never touched. |

### 1.4 Limitations (stated plainly)

- **A3, durable is 1.5.0 protocol 2 only.** A host whose `welcome.pi_durable` is not exactly `"1.5.0"`, or whose `welcome.protocol` is not 2, is refused with `unsupported_host_version` (version). A `view` or `ops` frame that does not match the shapes of section 6.4, or an op the applier refuses, is `protocol_error` (shape). The session shows that error; nothing is guessed. **Superseded by 10.8:** durable is no longer 1.5.0-only: `assertSupportedHost` accepts (protocol 1, pi_durable "1.3.0") and (protocol 2, pi_durable "1.5.0") and refuses every other pair with `unsupported_host_version`.
- **A4, pi sessions cannot be driven fully.** For a pi session: no abort (no `cpd session abort` exists), no steer, no model/thinking selection (the Tier decides, see 6.5), no compaction. Each is `{supported:false, reason}` in `capabilities`. Images and interactive dialogs work (11.3, 11.4). A queued-submission list does not exist (`queued` is always `[]`).
- Footer: pi sessions report cost and context % as `unknown` with a reason (a full-file scan is forbidden by 6.5; the window size of the model is not known). Durable cost is the cumulative cost of the Session's models (`docs['pi.usage']`, 6.4), so it keeps the cost of turns that a compaction removed from the transcript. No value is faked.
- Phase 1 does not render `cpd` session messages (`customType` `cpd`, `cpd-error`) or other custom message types: they are dropped. Only the four fold kinds of 3.2 render. **Superseded by 10.1:** `cpd` custom messages are no longer dropped: a session-message from `null` is a User message, every other displayed `cpd` message is a collapsed fold of fold kind `cpd`; `cpd-error`, `cpd-first-message`, `cpd-reminder` and `display === false` stay dropped, and the durable entry kind `cpd-chat` is a user item (10.2).
- One attached session per browser connection. No auth. Dev bench only (`vite serve`), never `vite build` output.
- No visual design (pro-d16).
- **Status bar, Phase 1 (pro-d18).** No cpd version reaches the web: `CockpitSnapshot` (6.3, 5) has no version field, so the StatusBar shows `cpd: unknown`. This is a stated limitation, not a wire change (unverified source, O19). The runner and state come from the selected session's `SessionNode.ref` in `cockpit.tree` (6.11), so a selected session absent from the tree (archived, or removed by a patch) shows `none`. The header and status bar show no other data: no host stats, workers summary or balloons (pro-d16, pro-d18).
  **Superseded by docs/ui-preview.md, Production and preview modes (item 6):** the version does reach the web (`StatusModel.version`, from `cpd --version` in `status.ts`), so the `cpd: unknown` limitation above no longer holds. It is shown as a bare `ghost` Badge, and preview mode adds an `outline` Badge `preview` and a drift pill.

## 2. Module map (the fixed file layout; no other source path is written)

| Module | Implementation | Tests |
|---|---|---|
| protocol | `web/src/protocol/domain.ts`, `wire.ts` (also `parseClientMessage`, 10.9), `cockpit.ts` (also `parseCockpitAction`, 10.9), `runtime.ts` (also `GitBoundary`, 10.9) | covered by the tests of the modules that parse (`state/connection.test.ts`, `server/hub/connection.test.ts`, `server/cockpit/*.test.ts`) |
| state | `web/src/state/transcript.ts`, `store.ts`, `connection.ts` | `transcript.test.ts`, `store.test.ts`, `connection.test.ts` |
| cockpit-srv | `web/src/server/cockpit/cli.ts`, `readmodel.ts`, `events.ts`, `actions.ts`, `stream.ts` (12), `settings.ts` (19), `status.ts`, `cost.ts`, `logs.ts`, `liveness.ts` | `readmodel.test.ts`, `events.test.ts`, `actions.test.ts`, `stream.test.ts` (12), `settings.test.ts` (19), `status.test.ts`, `cost.test.ts`, `logs.test.ts`, `argv-contract.test.ts` (18) |
| durable-srv | `web/src/server/durable/client.ts`, `frames.ts`, `ops.ts` (Phase 2, 10.8), `backend.ts` | `client.test.ts`, `frames.test.ts`, `ops.test.ts`, `backend.test.ts` |
| pi-srv | `web/src/server/pi/jsonl.ts`, `window.ts`, `watch.ts`, `backend.ts` | `jsonl.test.ts`, `window.test.ts`, `watch.test.ts`, `backend.test.ts` |
| hub-srv | `web/src/server/hub/connection.ts`, `registry.ts`, `web/src/server/plugin.ts` | `hub/connection.test.ts`, `hub/registry.test.ts`, `plugin.test.ts` |
| ui-cockpit | `web/src/ui/cockpit/ContextTree.tsx`, `PlanView.tsx`, `ManagerView.tsx`, `Inbox.tsx`, `Shell.tsx`, `Header.tsx`, `StatusBar.tsx` | `ContextTree.test.tsx`, `PlanView.test.tsx`, `Inbox.test.tsx`, `Shell.test.tsx`, `Header.test.tsx`, `StatusBar.test.tsx` |
| ui-chat | `web/src/ui/chat/Transcript.tsx`, `ToolRow.tsx`, `Folds.tsx`, `ChatHeader.tsx`, `Footer.tsx`, and Phase 2: `rows.ts` (10.4), `virtual.ts` (10.4), `Markdown.tsx` (10.6) | `Transcript.test.tsx`, `ToolRow.test.tsx`, `Folds.test.tsx`, `ChatHeader.test.tsx`, `Footer.test.tsx`, `rows.test.ts`, `virtual.test.ts`, `Markdown.test.tsx` |
| ui-composer | `web/src/ui/composer/attachments.ts`, `drafts.ts`, `Composer.tsx`, `TierSelector.tsx` | `attachments.test.ts`, `Composer.test.tsx` (also the draft tests of 6.9), `TierSelector.test.tsx` |
| ui-dialogs | `web/src/ui/dialogs/AskCard.tsx`, `PlanReviewCard.tsx`, `SettingsDialog.tsx` (17, 19), `DocsDialog.tsx`, `LogsDialog.tsx`, `LegendDialog.tsx` | `AskCard.test.tsx`, `PlanReviewCard.test.tsx`, `StatusDialogs.test.tsx` (17, 19), `DocsDialog.test.tsx` |
| app | `web/src/ui/App.tsx`, `ChatPane.tsx`, `web/src/main.tsx`, `web/src/ui/app.css` | `web/src/ui/App.test.tsx` |
| ui-chat (Phase 2) | see the ui-chat row | the Phase 2 pure modules `rows.ts`, `virtual.ts` and the `splitMarkdown` function are tested without a DOM |
| test helpers | `web/src/testing/render.tsx`, `web/src/testing/scheduler.ts` | (helpers; used by the tests above) |
| config | `web/vite.config.ts` (registers `react()` and the cpd plugin only; the Tailwind import is removed, the dependency stays installed because `package.json` is byte-identical) | none |

Test ids are `web/<file>.test.ts(x)::<title>`. Run recipe: `timeout 300 pnpm --filter cpd-web-client exec vitest run <repo-root-relative test file>`; typecheck `timeout 300 pnpm --filter cpd-web-client run typecheck`.

### 2.1 Conventions that hold in every file

- TypeScript strict, `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, `erasableSyntaxOnly` (`web/tsconfig.json`): no `enum`, no constructor parameter properties, `import type` for types, `.ts`/`.tsx` extensions in relative imports.
- Types are unions with a `kind` / `type` discriminator, never optional-flag bags that allow illegal states.
- Untrusted data is parsed once at an edge and then trusted: the edges are `parseClientMessage` (hub), `parseServerMessage` (browser), `parseCpd*` (cockpit `cli.ts`/`readmodel.ts`), `parseHostFrame` (`durable/client.ts`), `parseLine` (`pi/jsonl.ts`). A failed parse throws `WebError` (3.9); there is no default, no swallowed error.
- No module outside the named boundary touches the outside world: only `cockpit/cli.ts` spawns `cpd`; only `plugin.ts` constructs real boundaries (child_process, net, fs, watch).
- Disposables: every `start`/`subscribe`/`connect` returns a `Disposable`; `dispose()` is idempotent and a disposed object emits nothing afterwards.

## 3. `web/src/protocol/domain.ts`: domain types

Pure types plus the constants and `WebError` below. No I/O. Shared by browser and server.

### 3.1 Constants

```ts
export const WINDOW_SIZE = 40; // A5: tail and each loadEarlier page
export const IMAGE_MAX_COUNT = 4; // durable host limit, protocol.ts:19 (scout-durable §2)
export const IMAGE_MAX_TOTAL_BASE64 = 786_432; // sum of base64 `data.length`, protocol.ts:20 (scout-durable §2)
export const IMAGE_MIME_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const; // protocol.ts:21
export type ImageMime = (typeof IMAGE_MIME_TYPES)[number];
export const TEXT_INLINE_MAX_BYTES = 65_536; // Phase 1 choice: larger text files are rejected
export const PI_SEND_TEXT_MAX = 20_000; // `cpd session send` text is 1..20000 chars (scout-pi §6, `cpd session send --help`)
export const FOLD_TITLE_MAX = 160; // fold title cut, as pi/lib/render.ts oneLine (scout-pi §3)
export const TOOL_PREVIEW_LINES = 10; // compact-tools previewLines default (scout-pi §4)
```

### 3.2 Sessions

```ts
export type Runner = "pi" | "durable"; // `runner` of `cpd session list` (scout-cockpit §4)
export type SessionRole = "manager" | "lead" | "worker";
export type SessionState = "working" | "waiting" | "blocked" | "idle" | "stopped"; // scout-cockpit §1.1 `state`

export interface SessionRef {
  readonly id: string; // session id (uuid), the primary key everywhere
  readonly key: string; // `context`, `context:plan` or `context:plan:job` (scout-cockpit §4). A null `key` (a fork; unverified, O5) is replaced by the session `name`, else by `id`.
  readonly runner: Runner;
  readonly role: SessionRole;
  readonly state: SessionState;
  readonly title: string | null; // `title` of the row; null when none
  readonly file?: string; // pi session JSONL path (`file`); absent for durable and for a Worker whose file is not written yet
  readonly tier?: string; // e.g. "S3"
  readonly cwd?: string; // `cwd` of the row, used for the git branch
}
```

### 3.3 Transcript items

> **Amended by 12:** `AssistantItem.streaming` is also true for a pi live item (12.2), and the pi live items of 12 are ephemeral.

Item ids are stable strings of the form `<prefix>:<source id>`; the same source always yields the same id (idempotence).

```ts
export type ItemId = string;
// Prefixes: u (user), a (assistant), tool (tool, by callId), f (fold), c (compaction), orphan (orphan result, by callId), live (a streaming durable assistant item).
// pi source id = the JSONL entry `id`; durable source id = the pi-durable entry id (`EntryRecord.id`).

export type TranscriptAttachment =
  | { readonly kind: "image"; readonly mimeType: string; readonly dataBase64?: string } // pi-ai ImageContent {type:"image", data, mimeType}
  | { readonly kind: "text"; readonly name: string };

export interface UserItem {
  readonly kind: "user";
  readonly id: ItemId;
  readonly at?: string; // ISO timestamp of the entry when known
  readonly text: string; // text blocks joined with "\n"; a string `content` as is
  readonly attachments: readonly TranscriptAttachment[]; // image blocks of `content`, in order
}

export type AssistantBlock =
  | { readonly type: "text"; readonly text: string }
  | { readonly type: "thinking"; readonly text: string }; // `thinking` of a pi-ai ThinkingContent; `thinkingSignature` is dropped

export interface AssistantItem {
  readonly kind: "assistant";
  readonly id: ItemId;
  readonly at?: string;
  readonly blocks: readonly AssistantBlock[]; // order of the message `content`; toolCall blocks are NOT blocks (they are ToolItems). A `thinking` block whose `thinking` is "" is dropped (jsonl). A non-empty string `errorMessage` appends the text block `{ type: "text", text: "Error: " + errorMessage }` after the content blocks (pro-d435; jsonl, live `message_end`, durable `blocksOf`). A message with no text or thinking block and no non-empty `errorMessage` yields NO AssistantItem, only its ToolItems.
  readonly streaming: boolean; // true only while a durable run streams this message
  readonly model?: string; // message.model when present
}

export type ToolStatus = "running" | "done" | "failed";
export interface ToolResult {
  readonly text: string; // text blocks of the toolResult `content` joined with "\n"
  readonly isError: boolean;
  readonly durationMs?: number;
}
export interface ToolItem {
  readonly kind: "tool";
  readonly id: ItemId; // `tool:<callId>`
  readonly callId: string;
  readonly name: string; // read, edit, write, bash, codemode, mcp__*, ask_user_question, ...
  readonly args: Readonly<Record<string, unknown>>; // complete `arguments` (a JSON object)
  readonly result?: ToolResult;
  readonly status: ToolStatus; // running = no result yet; done = result with isError false; failed = isError true
  readonly diff?: string; // edit only: `details.patch` when it is a string, else `details.diff` when it is a string; absent otherwise (unverified in a real file, O3)
}

export type FoldKind = "cpd-context" | "cpd-plan-decision" | "cpd-plan-summary" | "cpd-skills" | "cpd"; // "cpd" added by 10.1 (superseded by 10.1: the older four-kind union)
export interface FoldItem {
  readonly kind: "fold";
  readonly id: ItemId;
  readonly at?: string;
  readonly foldKind: FoldKind;
  readonly title: string; // see the title rules below
  readonly body: string;
  readonly defaultOpen: boolean; // true ONLY for cpd-plan-summary (pi/lib/render.ts:212, scout-pi §3); false for the other three
}
// Fold title rules (customType -> title), scout-pi §3 `render.ts:74-97`:
//  cpd-context:       details.summary when a non-empty string, else the first line of body cut to FOLD_TITLE_MAX
//  cpd-plan-summary:  `Plan review <details.review> summary`
//  cpd-plan-decision: `Plan review <details.review> approved` when details.approved === true, else `... rejected`
//  cpd-skills:        the first line of body cut to FOLD_TITLE_MAX
// `details` is `details` of a pi `custom_message` line, or `data` of a durable entry of that kind.
// Fold kind "cpd" (10.1): title rules and defaultOpen false are in 10.1; the cpd fold is the only kind with `defaultOpen` false by construction, except the compaction-failure fold of 6.4 live tail, L6 (`defaultOpen` true).
// Dropped (no item): customType `cpd-first-message`, `cpd-reminder`, `cpd-error`, any message with display === false, and every other customType except `cpd` (superseded by 10.1: "and every other customType" no longer includes `cpd`, section 1.4).

export interface CompactionItem {
  readonly kind: "compaction";
  readonly id: ItemId;
  readonly at?: string;
  readonly summary: string;
  readonly tokensBefore?: number; // pi JSONL `tokensBefore` when present (unverified, O2)
}

export interface OrphanResultItem {
  readonly kind: "orphanResult"; // a toolResult whose toolCall is not in the loaded window
  readonly id: ItemId; // `orphan:<callId>`
  readonly callId: string;
  readonly name: string; // toolName of the result
  readonly result: ToolResult;
}

export type TranscriptItem = UserItem | AssistantItem | ToolItem | FoldItem | CompactionItem | OrphanResultItem;
```

### 3.4 Windows and cursors

```ts
export type Cursor = string; // opaque to the browser; echoed back in `session.loadEarlier`
// pi:   `pi:<byteOffset>`  the file offset of the first byte of the earliest loaded line
// durable: `dur:<id>`      the item id of the first item of the window in the server-side transcript (6.4 backend); `dur:` alone names the end of the cache (used when it holds no item)

export interface Window {
  readonly items: readonly TranscriptItem[]; // oldest first
  readonly hasEarlier: boolean;
  readonly cursor: Cursor | null; // pass as `before` to load earlier; null exactly when hasEarlier is false
}
```

Invariants: `cursor === null` iff `hasEarlier === false`; item ids are unique within a window; windows never repeat an item of an adjacent window.

### 3.5 Footer

```ts
export type Stat<T> = { readonly state: "known"; readonly value: T } | { readonly state: "unknown"; readonly reason: string };

export interface FooterStats {
  readonly model: Stat<string>; // `provider/modelId`
  readonly tier: Stat<string>;
  readonly thinking: Stat<string>; // thinking level, e.g. "high"; "off" is a value
  readonly contextPercent: Stat<number>; // 0..100
  readonly cost: Stat<number>; // USD: durable, the sum of docs['pi.usage'] models' cost.total (6.4, cumulative); pi, unknown (6.5)
  readonly branch: Stat<string>; // git branch of the session cwd
}
```

An unknown stat always has a human `reason`; the UI renders the explicit marker `unknown` plus the reason, never a blank or a fake value.

### 3.6 Capabilities

```ts
export type Support = { readonly supported: true } | { readonly supported: false; readonly reason: string };

export interface Capabilities {
  readonly images: Support & { readonly maxCount: number; readonly maxTotalBase64: number }; // limits present in both cases
  readonly steer: Support; // submit whenBusy "steer"
  readonly followUp: Support; // submit whenBusy "followUp"
  readonly abort: Support; // abort the running turn
  readonly abortSubmission: Support; // cancel one queued submission
  readonly compact: Support;
  readonly configure: {
    readonly model: Support;
    readonly thinking: Support;
    readonly tier: Support & { readonly options: readonly string[] }; // options: Phase 1 choice, see 6.5 (unverified, O7)
  };
  readonly dialogs: { readonly mode: "interactive" } | { readonly mode: "readonly"; readonly reason: string };
}
```

The two backends' values are fixed in section 8.

### 3.7 Submissions and models

```ts
export type WhenBusy = "followUp" | "steer"; // durable host also has "reject"; its semantics are not traced, so it is not offered
export interface QueuedSubmission {
  readonly clientSubmissionId: string;
  readonly submissionId?: number; // host submission_id when known (durable)
  readonly text: string; // first 200 chars of the submitted text
  readonly whenBusy: WhenBusy;
}
export interface ModelChoice {
  readonly model: string; // `provider/modelId`, the exact string `configure` takes (run.ts:473-479)
  readonly name: string;
  readonly thinkingLevels: readonly string[]; // `thinking_levels` of ModelInfo (run.ts:74)
  readonly contextWindow: number; // `context_window`
}
export type ConfigureChange =
  | { readonly kind: "model"; readonly model: string }
  | { readonly kind: "thinking"; readonly level: string | null } // null clears (host `thinking_level: null`, protocol.ts:44)
  | { readonly kind: "tier"; readonly tier: string };
```

### 3.8 Dialogs

```ts
export interface QuestionOption {
  readonly label: string; // <= 60 chars (types.ts MAX_LABEL_LENGTH, scout-pi §5a); reserved labels Other / Type something. / Next are not allowed
  readonly description: string;
  readonly preview?: string; // Markdown; single-select only
}
export interface Question {
  readonly header: string; // <= 16 chars
  readonly question: string;
  readonly options: readonly QuestionOption[]; // 2..4
  readonly multiSelect?: boolean; // default false
}

export type DialogBody =
  | { readonly method: "ask_user_question"; readonly questions: readonly Question[] } // 1..4
  | { readonly method: "cpd_plan_review"; readonly title: string; readonly summary: string };

export type Dialog = DialogBody & {
  readonly id: string; // durable: the held `ui_request.id`; pi: `call:<callId>` (ask_user_question) or `review:<review>` (plan review)
  readonly interactive: boolean; // false = read-only, no current producer; pi and durable dialogs are interactive (11.4); the UI renders no controls
};
export type DialogOutcome = "answered" | "cancelled" | "timeout" | "superseded" | "aborted"; // ui_resolved.outcome (protocol.ts)
```

Validation (`parseDialog`, applied at both edges): questions 1..4; each question `header` <= 16, options 2..4, `label` <= 60. A violation is `protocol_error` (server side, from the host) or `invalid_message` (on a wire message).

**Answer shapes**, scout-pi §5b-5c:

```ts
export interface QuestionAnswer {
  readonly questionIndex: number;
  readonly question: string;
  readonly kind: "option" | "custom" | "multi";
  readonly answer: string; // option: the label; custom: the typed text; multi: the optional custom text ("" when none)
  readonly selected?: readonly string[]; // multi: the chosen labels
  readonly notes?: string;
  readonly preview?: string;
}
export interface QuestionnaireResult {
  readonly answers: readonly QuestionAnswer[];
  readonly cancelled: boolean;
  readonly globalNote?: string;
  readonly unanswered?: readonly number[];
}

export type DialogResponse =
  | { readonly method: "ask_user_question"; readonly outcome: "answered"; readonly result: QuestionnaireResult } // cancelled false
  | { readonly method: "ask_user_question"; readonly outcome: "cancelled" }
  | { readonly method: "cpd_plan_review"; readonly outcome: "approve" }
  | { readonly method: "cpd_plan_review"; readonly outcome: "changes"; readonly feedback: string } // feedback non-empty after trim
  | { readonly method: "cpd_plan_review"; readonly outcome: "reject" }
  | { readonly method: "cpd_plan_review"; readonly outcome: "cancelled" };
```

**Plan-review mapping onto the host (derived from `pi/durable-host/plan-review.ts:36-38,126-137` and `dialogs.ts`, scout-pi §5d; nothing invented).** The durable host's dialog has TWO rows, `Approve` and `Change`; there is no Reject on the wire. The durable backend therefore maps:

| `outcome` | Host frames |
|---|---|
| `approve` | `ui_response {id, value: "Approve"}` |
| `changes` | `ui_response {id, value: "Change"}`; the host then issues a second `ui_request {method: "editor", title: "What should change?"}` (plan-review.ts:137); the backend answers THAT request automatically with `ui_response {id: <editor request id>, value: <feedback>}` (an editor answer is a `value`, dialogs.ts `editor`). An empty note would return the host to the select, so `feedback` must be non-empty. |
| `reject` | NOT a host frame. `cpd plan decide <review> --reject --plan <plan>` through the injected `CpdExec` (plan-review.ts `decide` runs exactly this; scout-pi §5d). The host then sees the decision and closes its dialog (`ui_resolved`). Unverified that the web User may call this hidden command (O9). The review id comes from the plan summary fold `details.review` (`review` of the last `cpd-plan-summary`); `--plan` from the session ref key (`context:plan`). |
| `cancelled` | `ui_response {id, cancelled: true}` (the host stays waiting: `Plan review r is still waiting`, plan-review.ts:130) |

The editor `ui_request` that follows `Change` is consumed by the backend and is never shown as a `session.dialog`. Pi sessions never accept a `DialogResponse` (`unsupported_capability`).

Mapping of a durable `ui_request` onto a `Dialog` (frames.ts):

| host request | Dialog |
|---|---|
| `method: "questionnaire"` (purpose `ask_user_question`) | `{method:"ask_user_question", questions}` from `questions: UiQuestion[]` |
| `method: "select"`, `purpose: "plan_review"`, options `["Approve","Change"]` | `{method:"cpd_plan_review", title, summary}`; `title` = `args.title` of the latest `cpd_plan_review` tool item of the cached transcript, else `"Plan review"`; `summary` = `body` of the latest `cpd-plan-summary` fold of the cached transcript, else `""`. Pairing is by transcript order because `tool_call_id` is never sent (scout-durable §7.6). |
| `method: "select"`, `purpose: "secret_approve"` or `"host_approve"`, options `["Allow once","Deny"]` | `{method:"ask_user_question", id, interactive: true, questions}` with one question: `header` `Approval`, `question` = the payload `title`, options `Allow once` and `Deny` (`description` `""`); the answer is mapped as below |
| `editor` after a `Change` answer | consumed (above) |
| any other `method`/`purpose` (`confirm`, `input`, a `select` other than the three above, or an approval select with other options) | `protocol_error` `unsupported dialog <method>/<purpose>`; the backend answers it with `ui_response {cancelled: true}` so the host is not stuck |

**The approval flag.** A dialog is an approval only when the host's `ui_request.purpose` is `secret_approve` or `host_approve` (docs/contract.md §6.15, the approval flag). No header, title or question text decides it: a `questionnaire` of `ask_user_question` is never an approval, whatever its `header` says. The `Approval` header in the table above is set on the Dialog this backend builds from an approval; it is never read as an input.

**Approval answers (`secret_approve`, `host_approve`).** The answer arrives as `DialogResponse` `ask_user_question` `answered`, and the backend reads `result.answers[0]`. It sends `ui_response {id, value: "Allow once"}` only when that answer is an option (`kind: "option"`) with `answer` `"Allow once"`. Every other outcome sends `ui_response {id, value: "Deny"}`: `Deny`, a custom answer, an unanswered question, and `cancelled`. Fail closed. The backend keeps the purpose of each held approval request, so `respond` sends this `{value}` for it and not the `{answer}` of other questionnaires.

### 3.9 Attachments and errors

```ts
export type Attachment =
  | { readonly kind: "image"; readonly name: string; readonly mimeType: ImageMime; readonly dataBase64: string }
  | { readonly kind: "text"; readonly name: string; readonly text: string };
export interface WireImage { readonly mimeType: ImageMime; readonly dataBase64: string } // carried on `session.submit`; text attachments are inlined into `text` before sending (6.9)

export type ErrorCode =
  | "protocol_error" // a frame/line/JSON did not match its schema (host frame, wire message from the server)
  | "unsupported_host_version" // durable host welcome.pi_durable !== "1.5.0" or welcome.protocol !== 2 (A3); or the hello is answered with error protocol_mismatch
  | "host_unreachable" // the durable socket cannot be opened (missing, refused, path too long)
  | "host_disconnected" // the socket closed or `bye` arrived while attached
  | "host_error" // the host answered `error`; details {hostCode, ...} carries the host ErrorCode (scout-durable §3)
  | "session_not_found" // id not in the cockpit snapshot / not attached
  | "session_file_missing" // a pi session whose transcript file did not appear after a submit (6.5 polling)
  | "session_file_unreadable" // stat/read of the pi file failed
  | "malformed_line" // a pi JSONL line is not valid JSON or has no `type`; details {line?, offset}
  | "cpd_failed" // a cpd command exited non-zero; details {cpdCode, exitCode, message}
  | "cpd_malformed_json" // cpd printed something that is not the expected JSON
  | "invalid_message" // a wire message failed validation
  | "unknown_action" // a cockpit action kind is not one of the listed kinds
  | "unsupported_capability" // the session's capabilities say no; message = the capability reason
  | "invalid_submission" // empty text with no images (16.2), or text longer than the backend allows
  | "tool_failed" // a tool result arrived with status "failed" (16.1); no toast is raised for it (16.1, amended by air-d77): a failed agent tool call shows as a failed tool row
  | "not_attached" // a session message names a session that is not the attached one
  | "internal";

export class WebError extends Error {
  readonly code: ErrorCode;
  readonly details?: Readonly<Record<string, unknown>>;
  constructor(code: ErrorCode, message: string, details?: Readonly<Record<string, unknown>>); // sets name "WebError"
}
export interface WireError { readonly code: ErrorCode; readonly message: string; readonly details?: Readonly<Record<string, unknown>> } // WebError serialised
export const toWireError: (e: unknown) => WireError; // WebError -> as is; anything else -> {code:"internal", message}
```

### 3.10 Completion (Lead badge)

Decisions pro-d422 and pro-d424; assumptions A2 to A8 of the Lead completion badge. Lead sessions only (`SessionRef.role === "lead"`).

```ts
export type CompletionLabel = "safe_to_archive" | "unanswered_ask" | "open_work" | "unrecorded_decision" | "unescalated_issue";
export type CompletionTone = "done" | "decision" | "blocked"; // the existing Badge Tone members (DESIGN.md): success=done, warn=decision, danger=blocked (pro-d424). No new Tone, no new CSS.
export interface CompletionScore {
  readonly label: CompletionLabel;
  readonly score: number; // integer 0..100
}
export interface Completion {
  readonly label: CompletionLabel;
  readonly text: string; // full display text: `<label text> · <score>%`
  readonly score: number; // integer 0..100
  readonly tone: CompletionTone;
  readonly others?: readonly CompletionScore[]; // the other four labels' scores, sorted high to low (rule below); absent when there are none
}
```

**Label table.** Shown as `<label text> · <score>%`. The tone rule and the score rule are per label. `p` is the classifier's `probabilities`, `c` its `confidence`; both are optional.

| label | label text | tone | score |
|---|---|---|---|
| `safe_to_archive` | `Safe to archive` | `done` when `score >= 80`, else `decision` | chosen: `round((p.safe_to_archive ?? c ?? 1) * 100)` |
| `unanswered_ask` | `Unanswered ask` | `decision` | any other chosen label: `round((p.safe_to_archive ?? (1 - (c ?? 0.5))) * 100)` |
| `open_work` | `Open work` | `decision` when `score >= 40`, else `blocked` | as above |
| `unrecorded_decision` | `Unrecorded decision` | `decision` | as above |
| `unescalated_issue` | `Unescalated bug` | `decision` | as above |

- The score is clamped to 0..100 and is always the archive-safety score, whichever label is chosen.
- kev-4b alone decides the label. There is no client-side override.
- **`others` (server, `mapAnswer`).** When the answer's `probabilities` is an object, `others` is every label of the five EXCEPT the chosen one whose probability is a finite number, as `{label, score: clamp(round(p * 100), 0, 100)}`, sorted by `score` descending. Ties keep the label order `safe_to_archive`, `unanswered_ask`, `open_work`, `unrecorded_decision`, `unescalated_issue`. The key is OMITTED (absent, not `[]`) when there are no `probabilities` or none of the other labels' probabilities is finite. `label`, `text`, `score` and `tone` are unchanged: the badge keeps showing the chosen label and the archive-safety score.
- **`others` (wire, `parseCompletion`).** `others` is optional. When present it must be an array of `{label, score}` where `label` is one of the five `CompletionLabel` values and `score` is a finite number; otherwise the parser refuses with `details.field` `others` (4.3). A parsed completion carries `others` only when the wire had it; no default is filled in, and the UI reads the array as given.

## 4. `web/src/protocol/wire.ts`: the browser/server wire

> **Amended by 15 (Plan `webui-ws`):** the wire is carried over one WebSocket (ADR 0015 option B, air-d46) as JSON text frames, not as Vite custom events; §4 adds the two heartbeat messages, `ping` (4.1) and `pong` (4.2). The endpoint, the Origin check and the heartbeat are stated in §15 and docs/contract.md §5.5.

Carried as JSON text frames over the one WebSocket of §15 (A1, amended by 15): the renderer's `createWebSocketTransport` sends one JSON text frame per `ClientMessage` and delivers one per `ServerMessage`; the server writes one per `ServerMessage` and parses one per incoming frame. Payloads are JSON-serialisable; every message is an object with a string `type`.

### 4.1 Client to server

> **Amended by 15:** adds the client message `ping` (the client's heartbeat; the server answers `pong` at once, 15.2).

```ts
import type { Attachment, Cursor, ConfigureChange, DialogResponse, WhenBusy, WireImage } from "./domain.ts";
import type { CockpitAction, SettingsChanges } from "./cockpit.ts";

export type ClientMessage =
  | { type: "ping" } // the client's wire heartbeat (15.2); the server answers { type: "pong" } at once, with no cockpit or session involvement
  | { type: "cockpit.subscribe" } // idempotent; the server answers with cockpit.snapshot (all panes)
  | { type: "cockpit.selectPlan"; planId: string | null } // choose the Plan shown in the Plan pane; null clears it
  | { type: "cockpit.action"; actionId: string; action: CockpitAction } // actionId: client correlation id, non-empty
  | { type: "session.attach"; sessionId: string } // attaches this connection to that session; an already attached session is detached first
  | { type: "session.detach" } // idempotent
  | { type: "session.loadEarlier"; sessionId: string; before: Cursor }
  | { type: "session.submit"; sessionId: string; clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy } // no `origin` field on the wire (11.9): the pi backend sends the text with `cpd session send --origin webui`, so the written `session-message` Event carries `data.origin: "webui"`; the durable backend adds none
  | { type: "session.abort"; sessionId: string }
  | { type: "session.abortSubmission"; sessionId: string; clientSubmissionId: string }
  | { type: "session.compact"; sessionId: string; instructions?: string }
  | { type: "session.configure"; sessionId: string; change: ConfigureChange }
  | { type: "session.listModels"; sessionId: string }
  | { type: "logs.read" } // answered with logs.result
  | { type: "settings.read" } // answered with settings.result (19): the Agent Tiers dialog's read
  | { type: "settings.save"; changes: SettingsChanges } // answered with settings.saved (19); only the values the User changed
  | { type: "dialog.respond"; sessionId: string; dialogId: string; response: DialogResponse };
```

`session.submit.images` holds only images (`WireImage`); text-file attachments are already inlined into `text` (6.9). `attachments` of the Composer (`Attachment`) never cross the wire.

### 4.2 Server to client

> **Amended by 12:** adds the server message `session.remove` (12.1 L1).
> **Amended by 15:** adds the server message `pong` (the answer to the client's `ping`; `parseServerMessage` accepts it, and it carries no session or cockpit state).
> **Amended by 20 (Plan `status-workers`):** the cockpit `status` pane rides this wire: `cockpit.snapshot` carries the optional `status` field and `cockpit.patch` carries `pane: "status"`, both already parsed and filed by the store. §20 states the pane, its fields and the serve-stream pane they come from.

```ts
import type { Capabilities, Cursor, Dialog, DialogOutcome, FooterStats, ModelChoice, QueuedSubmission, SessionRef, TranscriptItem, Window, WireError } from "./domain.ts";
import type { ContextTreeModel, PlanViewModel, InboxModel, ActionOutput, SettingsModel } from "./cockpit.ts";

export type CockpitPane = "tree" | "plan" | "inbox";
export type CockpitPaneData =
  | { pane: "tree"; data: ContextTreeModel }
  | { pane: "plan"; data: PlanViewModel | null } // null = no Plan selected
  | { pane: "inbox"; data: InboxModel };

export interface SessionMeta { ref: SessionRef; footer: FooterStats }

export type ServerMessage =
  // heartbeat
  | { type: "pong" } // the answer to the client's wire `ping` (15.2); no cockpit or session state
  // cockpit
  | { type: "cockpit.snapshot"; tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel } // all three panes; sent on subscribe
  | ({ type: "cockpit.patch" } & CockpitPaneData) // replaces ONE pane (an invalidation refresh); "patch" = per-pane replacement
  | { type: "cockpit.error"; error: WireError } // a refresh failed; the previous snapshot stays valid
  | { type: "action.result"; actionId: string; ok: true; output: ActionOutput }
  | { type: "action.result"; actionId: string; ok: false; error: WireError }
  | { type: "settings.result"; settings: SettingsModel } // the Agent Tiers dialog's read (19)
  | { type: "settings.saved"; ok: true } // the save ran to its last write (19)
  | { type: "settings.saved"; ok: false; error: WireError } // a refusal: shown once in the cockpit error toast (17.2, 19), never inline
  // session lifecycle
  | { type: "session.attached"; sessionId: string; capabilities: Capabilities; title: string | null; meta: SessionMeta }
  | { type: "session.window"; sessionId: string; mode: "tail" | "earlier"; window: Window } // tail replaces the transcript; earlier is prepended
  | { type: "session.append"; sessionId: string; items: TranscriptItem[] } // new items at the end
  | { type: "session.update"; sessionId: string; items: TranscriptItem[] } // full replacement of existing items by id
  | { type: "session.delta"; sessionId: string; itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string } // text appended to blocks[blockIndex] of an assistant item
  | { type: "session.status"; sessionId: string; busy: boolean; queued: QueuedSubmission[] }
  | { type: "session.submitAck"; sessionId: string; clientSubmissionId: string; queued: boolean; submissionId?: number }
  | { type: "session.footer"; sessionId: string; footer: FooterStats }
  | { type: "session.title"; sessionId: string; title: string | null }
  | { type: "session.models"; sessionId: string; models: ModelChoice[] }
  | { type: "session.dialog"; sessionId: string; dialog: Dialog }
  | { type: "session.dialogResolved"; sessionId: string; dialogId: string; outcome: DialogOutcome }
  | { type: "session.completion"; sessionId: string; completion: Completion } // Lead only; never null: the last non-null completion is replayed on attach, computed results follow asynchronously and may never arrive; `completion.others` is carried and parsed as 3.10 says (3.10)
  | { type: "session.error"; sessionId: string | null; clientSubmissionId?: string; error: WireError }; // sessionId: the attempted id of a session.attach error (e.g. session_not_found); null: not tied to a session (invalid_message)

export type SessionServerMessage = Extract<ServerMessage, { type: `session.${string}` }>; // what a SessionBackend emits
```

Message semantics (server behaviour):

- **Order on attach.** `session.attached`, then `session.window {mode:"tail"}`, then `session.footer`, `session.status`, then `session.dialog` for each open dialog, oldest first. Live messages follow. A message for a session other than the connection's attached one is never sent.
- `session.window mode "tail"` carries the last `WINDOW_SIZE` items (A5). `mode "earlier"` carries the page ending before the `before` cursor and a fresh `cursor`.
- `session.append` items never have ids already present; `session.update` items always have ids already sent. The reducers still merge by id in both cases (4.4).
- `session.delta` addresses `blocks[blockIndex]`: `blockIndex === blocks.length` creates a new block of `blockType`; a smaller index appends `text` to that block.
- `session.submitAck` answers each `session.submit` that the backend accepted: `queued: true` when the host queued it behind a running turn, else false. A rejected submit answers `session.error` with `clientSubmissionId` set.
- `session.status.busy` is true while a turn runs. `queued` lists accepted-but-not-started submissions (durable only; always `[]` for pi).
- A `session.dialog` with `interactive: false` is read-only (no current producer; pi and durable dialogs are interactive, 11.4).
- **Completion badge (`session.completion`, 3.10).** Sent only for a Lead session (`role: "lead"`); no other role receives it. The badge is always visible for a Lead session and shows the last non-null value, across navigation and while a new classification runs; a failed or slow classification never clears it (pro-d438). The server keeps the last non-null completion per session id: on Lead attach it replays that value at once, before any computed result; a session with no last value gets nothing on attach. Otherwise the completion is computed asynchronously: it may arrive later than any other message, and may never arrive. Triggers (A8): Lead attach, after `session.attached`; and a turn settle (`session.status` `busy` true -> false). Neither trigger blocks attach, rendering or message submission.
- A session is classified only when it has at least one transcript item (A5).
- `completion: Completion` sets the badge and becomes the session's last non-null value. The server never sends `completion: null`. `completion.others` (3.10), when present, is carried by the server and parsed by `parseCompletion`; the Lead badge shows it in a HoverCard (DESIGN-chat 1.4), never in the badge text. Fail open (pro-d369): an unreachable classifier, a 30 s timeout (on the Scheduler seam `b.scheduler`, never a real timer in tests), a non-zero exit, unparseable output or an unknown label yields no completion: nothing is sent and the badge keeps its last value. No error is shown and there is no config switch (A4).
- Cache and in-flight (A7): the key is `${sessionId}|${lastTranscriptItemId}|${planCursor}`, where `planCursor` is `PlanViewModel.cursor` from `cpd plan show`. In-flight computations are shared per key. A null result is not cached. A result is never sent to a closed connection or to a detached session.
- `action.result ok:true` carries `output` (section 5.4); `ok:false` carries the `cpd_failed` / `unknown_action` error.

### 4.3 Validation (parsed once at each edge)

```ts
export function parseClientMessage(raw: unknown): ClientMessage; // used ONLY by server/hub/connection.ts. Superseded by 10.9: declared in protocol/wire.ts (this section), imported by server/hub/connection.ts
export function parseServerMessage(raw: unknown): ServerMessage; // used ONLY by state/connection.ts
```

Both are pure and throw `WebError`:

- not an object, no string `type`, or an unknown `type` -> `invalid_message` (server messages: `protocol_error`);
- a missing field, a wrong primitive type, an empty `sessionId` / `clientSubmissionId` / `actionId` / `dialogId` -> same codes, `details.field` names the field;
- `session.submit`: `images.length <= IMAGE_MAX_COUNT`, every `mimeType` in `IMAGE_MIME_TYPES`, summed `dataBase64.length <= IMAGE_MAX_TOTAL_BASE64`, `whenBusy` in `followUp|steer`, `text` a string (emptiness with no images is the backend's `invalid_submission`; an image-only message is valid, 16.2); it carries no `origin` field (11.9);
- `session.configure`: exactly one `change.kind` of `model|thinking|tier`;
- `dialog.respond`: `response.method`/`outcome` pair from `DialogResponse`; `changes` needs `feedback.trim() !== ""`;
- `cockpit.action`: the action goes through `parseCockpitAction` (section 5.5), whose failures are `unknown_action` (unknown kind) and `invalid_message` (bad field of a known kind);
- `settings.save`: `changes` is parsed once at the boundary (19): every tier a `TIER` string, every switch a bool, and `concurrency`, when present, an integer in 1 to 16 inclusive; a bad value is `invalid_message` with `details.field` naming the field;
- `Dialog` objects, `Window`s (`cursor === null` iff `!hasEarlier`) and `TranscriptItem`s are checked structurally (kind discriminator, required fields, FoldItem `foldKind` in the five kinds (superseded by 10.1: the four kinds, `"cpd"` is the fifth)).

Extra unknown properties are ignored by the parsers and dropped from the parsed value. Inside the program the parsed types are trusted; no later code re-validates.

### 4.4 Reducer merge rule (used by `state/transcript.ts`)

`mergeById(items, incoming)`: for each incoming item in order, if an item with the same id exists it is replaced in place, else it is appended at the end. `session.update` for an id that is NOT in the loaded transcript is ignored (the item is outside the loaded window and arrives complete when its page loads); `session.append` follows `mergeById` (a duplicate id replaces).

**Orphan adoption.** When a `ToolItem` with `callId` X enters the transcript (by append, update or an `earlier` page) and an `OrphanResultItem` `orphan:X` exists, the orphan is removed and, if the tool item has no `result`, it receives the orphan's `result` and the matching `status`.

## 5. `web/src/protocol/cockpit.ts`: cockpit read models and actions

Derived from REAL captured JSON (`scout-cockpit §1`, cpd `0.21.102`, protocol 21). Every cpd JSON reply is `{"ok":true, ...}` or `{"ok":false,"error":{"code","message","details"}}` with a non-zero exit (`scout-cockpit §3` read-only refusals).

### 5.1 Context tree

Sources: `cpd context list` (contexts: `id, title, archived_at, records, open_questions, plans[], jira, owner`), `cpd plan list` (plans: `id, context, title, phase, archived_at, created_at, lead_session, counts{draft,queued,running,merged,failed,cancelled}, workflows[]`), `cpd session list` (sessions: `id, key, name, runner, role, state, state_label, place, tmux_pane, file, plan_lead, tier, title, context, plan, cwd, archived_at, attempt`), `cpd inbox list` (inbox rows, 5.3). Each reply also has `events_cursor` (`scout-cockpit §1.1`).

```ts
import type { SessionRef, SessionState } from "./domain.ts";

export type Urgency = "red" | "yellow" | "magenta" | "blue" | "cyan" | "white"; // seen live: red, magenta; the rest from docs/commands.md:3910 (unverified, O15)

export interface PlanCounts { draft: number; queued: number; running: number; merged: number; failed: number; cancelled: number }

export interface SessionDot {
  readonly state: SessionState;
  readonly attention: Urgency | null; // the most urgent open inbox item that fills this row's dot; always null while state === "working"
}

export interface SessionNode {
  readonly ref: SessionRef;
  readonly contextId: string;
  readonly planId: string | null; // `plan` of the row
  readonly jobKey: string | null; // third segment of `key` for a Worker; null otherwise
  readonly archived: boolean; // archived_at !== null
  readonly dot: SessionDot;
}
export interface PlanNode {
  readonly id: string;
  readonly contextId: string;
  readonly title: string;
  readonly phase: string; // grounding | plan | dispatch | deploy | done ... (opaque string)
  readonly archived: boolean;
  readonly counts: PlanCounts;
  readonly leadSessionId: string | null;
  readonly sessions: readonly SessionNode[]; // the Plan's sessions: the Lead first, then Workers in `session list` order
}
export interface ContextNode {
  readonly id: string;
  readonly title: string;
  readonly archived: boolean;
  readonly records: number;
  readonly openQuestions: number;
  readonly sessions: readonly SessionNode[]; // sessions of the Context with plan === null (Manager, forks)
  readonly plans: readonly PlanNode[];
}
export interface ContextTreeModel {
  readonly contexts: readonly ContextNode[];
  readonly cursor: number; // min of the events_cursor of the four commands
}
```

**Grouping and order (`buildTree`).**

1. A Context node per row of `context list`, in source order; non-archived Contexts first, archived last (stable within each group).
2. A Plan node per row of `plan list`, placed under the Context named by its `context`; live Plans first, ordered by `created_at` descending, archived Plans last. `counts` is copied as is.
3. A Session node per row of `session list` (its default listing excludes archived sessions: inferred from the `--archived` flag, unverified, O12): with `plan === null` it goes to `ContextNode.sessions`; otherwise to `PlanNode.sessions` of Plan `plan` in Context `context`, the Lead (`plan_lead === true`) first.
4. A Plan or Session whose Context (or Plan) is absent from the other command's reply (the four commands run at different instants) is attached to a synthesised node: a Context `{id, title: id, archived: false, records: 0, openQuestions: 0}` / a Plan with the row's own fields it lacks set to `phase: "unknown"`, zero counts, `leadSessionId: null`. This is explicit, never silent dropping.
5. `SessionRef.key` rule: `key` if a non-empty string, else `name` if a string, else `id`. The JSON of a durable session row is unverified (no durable Session exists on the machine, scout-cockpit §4; O4).

**Dot rule** (`docs/commands.md:5390,5399`, §17; Phase 1 implements the session-row part only):

- `state === "working"` -> `attention: null` (own state first).
- otherwise `attention` = the highest-ranked `urgency` among the open, visible inbox rows (those of `inbox list`; rows with `snoozed_until`, `dismissed_at` or `claimed_at` non-null are excluded) that are linked to the session. Rank order: `red > yellow > magenta > blue > cyan > white` (legend order, `docs/commands.md:3910`). A row is linked to a session when `row.session === session.id`; ADDITIONALLY, for a Plan's Lead (`plan_lead === true`), when `row.plan === session.plan` and `row.kind` is `plan-review`, `job-stuck`, `pr-closed`, or `pr-ready` with urgency `red` or `yellow` (cyan/white `pr-ready` fill no dot, `commands.md:5390`).
- no linked row -> `attention: null`.

### 5.2 Plan view

Source: `cpd plan show --plan <id>` (`scout-cockpit §1.2`): `plan{id,context,title,phase,archived_at,lead_session}`, `counts`, `jobs[]`, `workflows[]`, `review{id,status,current,feedback}`, `escalations[]`, `events_cursor`.

```ts
export interface AttemptView {
  readonly id: string; readonly number: number; readonly tier: string;
  readonly status: string; readonly outcome: string | null; readonly cause: string | null;
  readonly workerSession: string | null; // worker_session
}
export interface JobView {
  readonly key: string; readonly title: string; readonly kind: string; readonly status: string; // queued|running|merged|failed|cancelled|...
  readonly tier: string; readonly needs: readonly string[];
  readonly workflow: string | null; // key of the Workflow whose `jobs` lists this Job; explore Jobs: null
  readonly attempts: readonly AttemptView[];
}
export interface WorkflowView {
  readonly key: string; readonly title: string; readonly repo: string; readonly base: string;
  readonly status: string; // open|merged|cancelled|waiting|...
  readonly needs: readonly string[];
  readonly jobKeys: readonly string[]; // `jobs` of the Workflow row (keys), in source order
  readonly pr: { readonly number: number; readonly url: string; readonly state: string | null; readonly checks: string | null; readonly mergeable: string | null } | null; // from `pr` + `pull_request` (state, checks, mergeable)
}
export interface PlanViewModel {
  readonly planId: string; readonly contextId: string; readonly title: string; readonly phase: string;
  readonly counts: PlanCounts;
  readonly review: { readonly id: string; readonly status: string; readonly current: boolean } | null;
  readonly openEscalations: number; // escalations.length (open only)
  readonly workflows: readonly WorkflowView[];
  readonly jobs: readonly JobView[]; // all Jobs of the Plan in `seq` order
  readonly cursor: number; // events_cursor
}
```

Notes: live Job objects carry no `workflow` key although `docs/commands.md:883` says so (`scout-cockpit §1.2` "Verified divergence"); the Job's Workflow is therefore taken from membership in `workflows[].jobs`; a Job in no Workflow (explore) has `workflow: null`. A Job whose `needs` is absent is `[]`.

### 5.3 Attention Inbox

Source: `cpd inbox list` (`scout-cockpit §1.3`): `items[]{id,kind,label,urgency,context,plan,job,session,url,opened_at,age_seconds,snoozed_until,dismissed_at,claimed_at,event,record,subject}`, `engine{running}`, `hidden{claimed,dismissed,snoozed}`, `events_cursor`.

```ts
export interface InboxRowAction { readonly label: string; readonly action: CockpitAction }
export interface InboxRow {
  readonly id: number;
  readonly kind: string; // seen: pr-ready, dialog, unread, curate-retire; documented: plan-review, loop-guard, drafts-parked, pr-closed, review-requested, deploy, engine-down (scout-cockpit §1.3)
  readonly label: string; // may contain "\n"
  readonly urgency: Urgency;
  readonly contextId: string | null; readonly planId: string | null; readonly jobKey: string | null;
  readonly sessionId: string | null; // `session`, the chat target when set
  readonly url: string | null;
  readonly openedAt: string; readonly ageSeconds: number;
  readonly actions: readonly InboxRowAction[];
}
export interface InboxModel {
  readonly openCount: number; // items.length (the list holds open, visible items only)
  readonly engineRunning: boolean;
  readonly hidden: { readonly claimed: number; readonly dismissed: number; readonly snoozed: number };
  readonly rows: readonly InboxRow[]; // source order (rank order of cpd)
  readonly cursor: number;
}
```

Row actions (`buildInbox`), by kind (commands in `scout-cockpit §3`):

| Rows | Actions (label -> action) |
|---|---|
| every row with a numeric `id` | `Snooze 15m` -> `inbox.snooze {duration:"15m"}`; `Snooze 1h` -> `{duration:"1h"}`; `Snooze until tomorrow` -> `{duration:"tomorrow"}`; `Dismiss` -> `inbox.dismiss` |
| kind `review-requested` | additionally `Claim` -> `inbox.claim` (first) |
| kind `deploy` | additionally `Approve` -> `inbox.approve` (first) |
| synthetic `engine-down` (`id: null`, not stored) | no actions; `id` is `-1` and the row is rendered without controls |

### 5.4 Actions

```ts
export type SnoozeDuration = "15m" | "1h" | "tomorrow";
export type CockpitAction =
  | { kind: "inbox.snooze"; itemId: number; duration: SnoozeDuration }
  | { kind: "inbox.dismiss"; itemId: number }
  | { kind: "inbox.claim"; itemId: number }
  | { kind: "inbox.approve"; itemId: number }
  | { kind: "pr.post"; planId: string; workflow: string }
  | { kind: "session.retier"; sessionId: string; tier: string }
  | { kind: "plan.reject"; planId: string; review: string }
  | { kind: "context.retire"; contextId: string; approve: string; reject: string } // 11.1
  | { kind: "error.report"; target: "cpd" | "host"; code: string; message: string }; // 11.7
export type ActionOutput = Readonly<Record<string, unknown>>; // the cpd reply object, without `ok`
```

| Action | cpd args (exact, `cpd` implied) | Source |
|---|---|---|
| `inbox.snooze` | `["inbox","snooze",String(itemId),duration]` | scout-cockpit §3 |
| `inbox.dismiss` | `["inbox","dismiss",String(itemId)]` | scout-cockpit §3 |
| `inbox.claim` | `["inbox","claim",String(itemId)]` | scout-cockpit §3 |
| `inbox.approve` | `["inbox","approve",String(itemId)]` | scout-cockpit §3 |
| `pr.post` | `["pr","post","--workflow",workflow,"--plan",planId]` | `docs/commands.md:2482` and `cpd pr post --help` (run this attempt). Caller is the Lead (`docs/commands.md:275`); allowed for the web User: unverified (O10) |
| `session.retier` | `["session","retier",sessionId,"--tier",tier]` | scout-cockpit §3 |
| `plan.reject` | `["plan","decide",review,"--reject","--plan",planId]` | scout-cockpit §3 (internal command; unverified that the web User may call it, O9) |
| `context.retire` | `["context","retire","--context=<contextId>"]` plus `--approve=<approve>` and `--reject=<reject>` each only when non-empty (ids are comma-separated) | 11.1 (approved preview, `actions.ts`) |
| `error.report` | `["session","send","<target>:manager","--unblocks","WebUI error toast (<code>)","--","Error toast reported from WebUI:\n- code: <code>\n- message: <message>"]`; `<target>` is `cpd` or `host`, and `\n` is a newline in the element. The cpd command is `cpd session send <target>:manager --unblocks 'WebUI error toast (<code>)' -- 'Error toast reported from WebUI:\n- code: <code>\n- message: <message>'` | 11.7 (assumption A5, pro-d452) |

`error.report` parses as one kind: `target` must be `"cpd"` or `"host"`; `code` and `message` must be strings (else `invalid_message` with `details.field`, as 5.5).

Pi prompt sending is not a cockpit action: it is `sessionSendArgs` (6.3, `actions.ts`).

### 5.5 Parser

```ts
export function parseCockpitAction(raw: unknown): CockpitAction; // superseded by 10.9: lives in protocol/cockpit.ts and is imported by server/cockpit/actions.ts
```
Unknown or missing `kind` -> `WebError("unknown_action")`; a known kind with a missing or mistyped field -> `WebError("invalid_message", ..., {field})`. `itemId` is an integer >= 0 (the synthetic `-1` is rejected); `duration` in the three values; strings non-empty.

---

## 6. Modules: signatures, boundaries, behaviours

Each subsection lists every exported symbol, the injected boundaries, and the behaviours a test is written from. "Boundary" = an interface the test fakes.

### 6.1 `web/src/protocol/runtime.ts`: shared boundaries

```ts
export interface Disposable { dispose(): void } // idempotent

export interface Scheduler {
  now(): number; // epoch milliseconds
  after(ms: number, run: () => void): Disposable; // one-shot; dispose cancels
}

export interface Transport {
  connect(): void; // begins ONE connection attempt; the outcome arrives via onStatus
  send(raw: unknown): void; // throws WebError("internal") when not open
  close(): void; // idempotent; after close no status or message event fires and connect() is a no-op
  onMessage(handler: (raw: unknown) => void): Disposable;
  onStatus(handler: (status: "open" | "closed") => void): Disposable;
}

// Session backends (server side; implemented by 6.4 and 6.5, consumed by 6.6)
export interface ResolvedSession {
  readonly ref: SessionRef;
  readonly file?: string; // pi: the session JSONL path
  readonly socketPath?: string; // durable: `$CPD_HOME/run/host/<id>.sock`
  readonly tierOptions: readonly string[]; // distinct `tier` values of the cockpit snapshot's sessions plus the session's own, sorted (Phase 1 choice, O7)
} // built by the registry (6.6)
export type SessionEmit = (message: SessionServerMessage) => void; // a backend never calls it after dispose()
export interface GitBoundary { branch(cwd: string): Promise<string | null> } // `git -C <cwd> rev-parse --abbrev-ref HEAD`; null when not a repository or detached

export interface SessionHandle {
  readonly capabilities: Capabilities;
  loadEarlier(before: Cursor): Promise<void>; // emits session.window mode "earlier"
  submit(input: { clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy }): Promise<void>; // emits session.submitAck, or throws a WebError
  abort(): Promise<void>;
  abortSubmission(clientSubmissionId: string): Promise<void>;
  compact(instructions?: string): Promise<void>;
  configure(change: ConfigureChange): Promise<void>;
  listModels(): Promise<void>; // emits session.models
  respond(dialogId: string, response: DialogResponse): Promise<void>;
  dispose(): Promise<void>; // idempotent; stops streams, closes sockets/watchers; nothing is emitted afterwards
}
// attach emits, in order: session.attached, session.window (tail), session.footer, session.status, one session.dialog per open dialog (4.2)
export interface SessionBackend { attach(session: ResolvedSession, emit: SessionEmit): Promise<SessionHandle> }
```

The real browser transport (amended by 15.1, Plan `webui-ws`): `main.tsx` mounts `createWebSocketTransport` (15.1), not the `import.meta.hot` transport — `send` = `import.meta.hot.send("cpd:c2s", raw)`; `onMessage` = `import.meta.hot.on("cpd:s2c", ...)`; status from `import.meta.hot.on("vite:ws:connect" | "vite:ws:disconnect", ...)` (declared in `vite/types/customEvent.d.ts:16-17`; their runtime timing was unverified, O11) — that hot transport is deleted with the `cpd:c2s`/`cpd:s2c` route. The real `Scheduler` is `{ now: Date.now, after: (ms, run) => setTimeout }`, constructed in `main.tsx` and `plugin.ts` only.

Test helpers (`web/src/testing/scheduler.ts`): `class FakeScheduler implements Scheduler { advance(ms: number): void /* runs due timers in time order, synchronously */; pending(): number; }` and `class FakeTransport implements Transport { sent: unknown[]; connects: number; emitMessage(raw): void; emitStatus(s): void; closed: boolean }`. `web/src/testing/render.tsx`: `render(element): { container: HTMLElement; rerender(el): void; unmount(): void }` using `createRoot` + `act`, and `click(el)`, `type(el, text)` helpers that dispatch real DOM events inside `act`.

### 6.2 State module `web/src/state/`

> **Superseded by 10.3:** `PendingSubmission.state` gains `"sent"`; `session.submitAck` no longer always removes the row; `TranscriptState` gains `tailEpoch`; `setTail` takes an optional previous state; `applyDelta` searches from the end; `StoreAction` gains `serverBatch`; `connection.ts` coalesces frames (`FRAME_MS`) and schedules `PENDING_ECHO_MS`. Where the text below says "removes the pending submission" for `session.submitAck` or "`setTail(window)`", 10.3 wins.

#### `transcript.ts` (pure)

```ts
export interface TranscriptState { readonly items: readonly TranscriptItem[]; readonly hasEarlier: boolean; readonly cursor: Cursor | null }
export const emptyTranscript: TranscriptState; // { items: [], hasEarlier: false, cursor: null }
export function setTail(window: Window): TranscriptState; // items, hasEarlier, cursor from the window
export function prependEarlier(state: TranscriptState, window: Window): TranscriptState; // window.items prepended in order; items whose id already exists are skipped (no duplicates); hasEarlier/cursor from the window; orphan adoption (4.4)
export function appendItems(state: TranscriptState, items: readonly TranscriptItem[]): TranscriptState; // mergeById, orphan adoption
export function updateItems(state: TranscriptState, items: readonly TranscriptItem[]): TranscriptState; // replace by id in place; unknown ids ignored; orphan adoption
export function applyDelta(state: TranscriptState, d: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }): TranscriptState;
```

Behaviours:

- `setTail` sets items and `hasEarlier`/`cursor` exactly from the window (a tail replaces the previous transcript).
- `prependEarlier` puts the older items before the existing ones, keeps order, never duplicates an id, and takes `hasEarlier`/`cursor` from the new window.
- `appendItems`/`updateItems` merge by id as in 4.4; an update of an id not loaded is ignored; ordering of existing items never changes.
- `applyDelta`: target item must be an `assistant` item; `blockIndex < blocks.length` appends `text` to that block (block type unchanged); `blockIndex === blocks.length` adds a block `{type: blockType, text}`; `blockIndex > blocks.length` or a non-assistant target throws `WebError("protocol_error")`; an unknown `itemId` returns the state unchanged (item outside the window, same rule as update).
- A `ToolItem` update carrying `result` and status `done|failed` replaces the tool item in place ("tool result updates its tool item").
- All functions are pure: the input state is never mutated, and an unchanged result returns the same object where nothing changed.

#### `store.ts`

```ts
export type ConnectionStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface PendingSubmission {
  readonly clientSubmissionId: string; readonly text: string; readonly imageCount: number; readonly whenBusy: WhenBusy;
  readonly state: "sending"; // Amended by 16.1 (air-d44): no "failed" state — a session.error with a clientSubmissionId drops the row and shows its error once in the cockpit error toast (11.7), so the row never carries an error; 10.3 adds "sent"
}
export interface SessionView {
  readonly id: string;
  readonly attached: boolean; // false from select() until session.attached arrives
  readonly ref: SessionRef | null; readonly title: string | null; readonly footer: FooterStats | null; readonly capabilities: Capabilities | null;
  readonly transcript: TranscriptState;
  readonly busy: boolean; readonly queued: readonly QueuedSubmission[];
  readonly pending: readonly PendingSubmission[];
  readonly dialog: Dialog | null; // the active dialog
  readonly completion: Completion | null; // Lead completion badge (3.10); the last non-null completion of this session (`completions[id]`, or the replay on attach); null only while none is known; set only by a non-null session.completion
  readonly models: readonly ModelChoice[] | null;
  readonly error: WireError | null; // the session.error without a clientSubmissionId for the selected id: the attach error the status line shows (6.2 Retry, 6.11 ChatPane)
}
export interface AppState {
  readonly connection: ConnectionStatus; readonly retryAttempt: number; // reconnect attempts since the last open
  readonly cockpit: { readonly tree: ContextTreeModel | null; readonly plan: PlanViewModel | null; readonly inbox: InboxModel | null; readonly selectedPlanId: string | null; readonly error: WireError | null };
  readonly selectedSessionId: string | null;
  readonly session: SessionView | null; // null iff selectedSessionId is null
  readonly completions: Readonly<Record<string, Completion>>; // last non-null Completion per session id (3.10); initial {}; a null completion never clears it
  readonly lastError: WireError | null; // last local protocol violation or action failure (action.result ok:false)
}

export type StoreAction =
  | { type: "connection"; status: ConnectionStatus; retryAttempt?: number }
  | { type: "server"; message: ServerMessage }
  | { type: "selectSession"; sessionId: string | null }
  | { type: "selectPlan"; planId: string | null }
  | { type: "submitPending"; submission: Omit<PendingSubmission, "state"> } // adds state "sending"
  | { type: "dismissPending"; clientSubmissionId: string };

export const initialState: AppState;
export function reduce(state: AppState, action: StoreAction): AppState; // pure

export interface Store { getState(): AppState; subscribe(listener: () => void): () => void; dispatch(action: StoreAction): void }
export function createStore(initial?: AppState): Store; // listeners run synchronously after each state change; a no-op action (reduce returned the same object) notifies nobody
export function useStore<T>(store: Store, select: (s: AppState) => T): T; // React `useSyncExternalStore`; selector result must be referentially stable between unchanged states
```

Reducer behaviours (every ServerMessage is `{type:"server"}`):

- `cockpit.snapshot` sets the three panes and clears `cockpit.error`; `cockpit.patch` replaces exactly the named pane; `cockpit.error` sets `cockpit.error` and keeps the panes. (A failed attach is retried on a tree update by `connection.ts`, 6.2 below.)
- Optimistic Inbox removal (pro-d426): each `state.optimistic[actionId]` entry is `{before, after, subject}`, where `subject?: string` is the removed row's subject (e.g. `pr:clayton-duarte/cpd#891`). A `cockpit.snapshot`, and a `cockpit.patch` with `pane: "inbox"`, while any optimistic action is in flight, sets `cockpit.inbox` to `syncOptimisticInbox(incoming, state.optimistic)`: every in-flight optimistic subject is filtered out of `incoming`, and each in-flight entry gets `before = incoming` and `after = filtered`. So every in-flight removal is re-applied across a refresh and stays hidden.
- A `cockpit.patch` with `pane: "inbox"` whose `data.cursor < state.cockpit.inbox.cursor` is ignored (state unchanged): a newer Inbox is never replaced by an older one.
- `selectSession` with an id: `selectedSessionId = id`, `session = { id, attached:false, completion: state.completions[id] ?? null, everything else empty/null, transcript: emptyTranscript }`; the same id again is a no-op only while `session.attached` is true, and when it is not attached the session is reset to this same empty state; `null` clears both. (The detach/attach messages are sent by `connection.ts`, 6.2 below, not by the reducer.)
- A server `session.*` message whose `sessionId` is not `selectedSessionId` is ignored (a late message of a detached session never lands).
- `session.attached` sets `attached:true`, `capabilities`, `title`, `ref`, `footer` from `meta`, and clears `error`. `session.window` mode `tail` -> `setTail`; mode `earlier` -> `prependEarlier`. `session.append` / `session.update` / `session.delta` -> the matching `transcript.ts` function (a thrown `WebError` is caught by `reduce`, stored in `lastError`, and leaves the state otherwise unchanged).
- `session.status` sets `busy` and `queued`. `session.footer` sets `footer`; `session.title` sets `title` (the header shows `title`; the footer and title also come from `session.attached.meta`).
- `session.submitAck` (not queued) marks the pending submission with that `clientSubmissionId` `state:"sent"`, or removes it when its echo is already in the transcript; a queued ack removes it (11.2). `session.error` with `clientSubmissionId` marks that pending submission `state:"failed"` with the error; without it sets `session.error` when its `sessionId` is the selected id (and, when `sessionId` is null, `lastError`). **Amended by 16.1 (air-d44):** a failed submission's row is removed from the transcript instead of being marked `failed`, and its error is shown once in the cockpit error toast (11.7); no inline text stays in the chat.
- `submitPending` adds a `sending` submission (with `at` and `images`, 11.2); `dismissPending` removes one.
- `session.dialog` sets `dialog`; `session.dialogResolved` clears `dialog` only when its `dialogId` equals the active dialog's id; `session.models` sets `models`.
- `session.completion` with a non-null `completion` for the attached session sets `completion` and `completions[sessionId]`. A `null` completion changes nothing: it never clears `completion` or `completions`. `completions` is never reset by navigation, so `selectSession` shows the last known value (`completions[id] ?? null`) until a newer one arrives.
- `action.result ok:false` sets `lastError` and removes the id from `pendingActionIds`; `ok:true` removes it from `pendingActionIds` and adds it to `completedActionIds` (11.2). The panes refresh through events. Its optimistic rollback (pro-d426) is `syncOptimisticInbox(record.before, remaining)`, where `remaining` is every other in-flight `state.optimistic` entry: the other in-flight removals stay hidden, and `record.before` is the latest server Inbox.
- `connection` sets `connection` and `retryAttempt`.

#### `connection.ts`

```ts
export interface ConnectionOptions {
  transport: Transport; scheduler: Scheduler; store: Store;
  newId?: () => string; // client ids; default: a counter-based `c<n>` generator, injectable for tests
  backoff?: { baseMs: number; maxMs: number }; // Phase 1 choice default { baseMs: 500, maxMs: 10_000 }; delay(n) = min(maxMs, baseMs * 2^n), n = retryAttempt starting at 0
}
export interface SubmitInput { text: string; images: WireImage[]; whenBusy: WhenBusy }
export interface Connection {
  start(): void;
  selectSession(sessionId: string | null): void;
  selectPlan(planId: string | null): void;
  loadEarlier(): void;
  submit(input: SubmitInput): string; // returns the clientSubmissionId
  abort(): void; abortSubmission(clientSubmissionId: string): void; compact(instructions?: string): void;
  configure(change: ConfigureChange): void; listModels(): void;
  respond(dialogId: string, response: DialogResponse): void;
  action(action: CockpitAction): string; // returns the actionId
  close(): void; // stops reconnecting, closes the transport; idempotent
}
export function createConnection(options: ConnectionOptions): Connection;
```

Behaviours:

- `start()`: dispatches `connection connecting`, calls `transport.connect()`. On status `open`: dispatch `open` with `retryAttempt: 0`, send `cockpit.subscribe`, then send `cockpit.selectPlan` when a Plan is selected, then `session.attach` for the selected session if any (connect subscribes to the cockpit and reattaches the selected session).
- Every inbound raw message goes through `parseServerMessage` once. A parsed message is dispatched as `{type:"server", message}`. An invalid message is never dropped silently: the connection dispatches `{type:"server", message:{type:"session.error", sessionId:null, error: <the protocol_error WireError>}}` (the reducer stores it in `lastError`) and stays open.
- `selectSession(id)`: dispatches `selectSession`; if a session was selected before, sends `session.detach` first; then, for a non-null id, `session.attach {sessionId}`. Selecting the id that is already selected sends nothing while `session.attached` is true; when it is selected but not attached, the reducer resets the session (808) and `session.attach {sessionId}` is sent again, with no `session.detach` (that attach never succeeded).
- Attach retry: when a `cockpit.snapshot`, or a `cockpit.patch` with `pane: "tree"`, arrives while the connection is `open`, a session is selected, `session.attached` is not true and `session.error.code` is `session_not_found`, `session.attach {sessionId}` is sent again for the selected id. This lets a session that appears in the tree after its attach was refused attach on the next tree update.
- **Retry.** Any other attach error of the selected session (every code but `session_not_found`, e.g. `host_unreachable` after the 15000 ms window of 6.4 backend) is not retried by the connection. It stays in `session.error` and the status line of 6.11 (ChatPane) shows it with a `Retry` action, which calls `selectSession(<the selected id>)`: the same-id rule above (reset to the empty state and `session.attach {sessionId}` again, no `session.detach`). The automatic retry above stays the only one (pro-d415).
- `loadEarlier()`: sends `session.loadEarlier {sessionId, before: transcript.cursor}` only when `hasEarlier` and a session is selected; otherwise no message.
- `submit(input)`: creates `clientSubmissionId` via `newId`, dispatches `submitPending` (`state:"sending"`, `imageCount = images.length`, `images`, `at`), sends `session.submit`, returns the id. Ack/failure arrive as server messages (reducer, above). Without a selected session: throws `WebError("not_attached")`.
- Other session methods send the matching `ClientMessage` with the selected `sessionId`, or throw `WebError("not_attached")` with no selected session.
- Status `closed` (not caused by `close()`): dispatch `connection reconnecting` with `retryAttempt: n`, schedule `transport.connect()` after `delay(n)` through `scheduler.after`; each further `closed` status increases `n`; `open` resets `n` to 0. Only one reconnect timer is pending at a time.
- `close()`: dispatches `connection closed`, cancels the pending reconnect timer, calls `transport.close()`; later status events do not schedule anything.
- `send` failures (`transport.send` throwing) propagate to the caller of the method; the store is not modified by a failed send except `submitPending`, which the failed `submit` removes again by `dismissPending`.

### 6.3 cockpit-srv `web/src/server/cockpit/`

#### `cli.ts`: the ONLY place that runs `cpd`

```ts
export interface CpdResult { readonly stdout: string; readonly stderr: string; readonly exitCode: number }
export interface CpdExec { run(args: readonly string[]): Promise<CpdResult> } // spawns `cpd <args>`; resolves for ANY exit code; rejects only when the process cannot be spawned

export interface FollowProcess {
  onLine(handler: (line: string) => void): Disposable; // one call per stdout line, newline stripped, partial lines are buffered until complete
  onExit(handler: (info: { code: number | null; stderr: string }) => void): Disposable;
  kill(): void; // idempotent
}
export interface FollowBoundary { start(args: readonly string[]): FollowProcess } // spawns `cpd <args>`, long-lived

export interface Cpd {
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>>; // the parsed reply object of a successful command
}
export function createCpd(exec: CpdExec): Cpd;
export function followArgs(since: number): readonly string[]; // ["events","follow","--since",String(since)]
```

`Cpd.json` behaviours (parsed once, at this boundary):

- exit 0 and stdout parses to an object with `ok === true` -> resolves that object.
- exit non-zero with stdout a JSON `{ok:false, error:{code, message, details?}}` -> `WebError("cpd_failed", message, {cpdCode: error.code, exitCode, details})`.
- exit non-zero with non-JSON stdout -> `cpd_failed` with `cpdCode: "no_json"`, `message` = trimmed stderr (or `cpd exited <n>`).
- exit 0 with `ok:false`, or stdout that is not a JSON object, or an object without `ok === true` -> `cpd_malformed_json` with `details.args`.
- `exec.run` rejecting -> `cpd_failed` with `cpdCode: "spawn_failed"`.

#### `readmodel.ts`

```ts
export type PaneSet = ReadonlySet<CockpitPane>;
export interface PaneUpdate { tree?: ContextTreeModel; plan?: PlanViewModel | null; inbox?: InboxModel }
export interface ReadModel {
  loadAll(planId: string | null): Promise<{ tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel }>;
  loadPanes(panes: PaneSet, planId: string | null): Promise<PaneUpdate>; // runs each needed command at most once (below)
}
export function createReadModel(cpd: Cpd): ReadModel;

// pure builders (exported for tests); inputs are the already-parsed cpd reply objects
export function parseContextList(reply: Readonly<Record<string, unknown>>): RawContext[];
export function parsePlanList(reply: ...): RawPlan[];
export function parseSessionList(reply: ...): RawSession[];
export function parseInboxList(reply: ...): { rows: RawInboxItem[]; engineRunning: boolean; hidden: {...}; cursor: number };
export function parsePlanShow(reply: ...): RawPlanShow;
export function buildTree(input: { contexts: RawContext[]; plans: RawPlan[]; sessions: RawSession[]; inbox: RawInboxItem[]; cursor: number }): ContextTreeModel;
export function buildPlanView(raw: RawPlanShow): PlanViewModel;
export function buildInbox(parsed: ReturnType<typeof parseInboxList>): InboxModel;
```

The `Raw*` types are the minimal required-field shapes of 5.1-5.3 (snake_case names as in the JSON). Parsers throw `WebError("cpd_malformed_json", msg, {command, field})` for a missing required field or wrong primitive; required fields: context `id,title,archived_at,records,open_questions`; plan `id,context,title,phase,archived_at,created_at,counts(6 numbers),lead_session`; session `id,runner(in pi|durable),role(in manager|lead|worker),state(in the five states),context,plan,plan_lead,archived_at`, and `file,tier,title,cwd,key,name` each `string|null`; inbox item `id(number or null for synthetic),kind,label,urgency,opened_at,age_seconds,context,plan,job,session,url,snoozed_until,dismissed_at,claimed_at`; plan show `plan.id,plan.context,plan.title,plan.phase,counts,jobs[],workflows[],review(null|object),escalations[]`; every reply `events_cursor` (number). Unknown extra fields are ignored. Strings longer than the JSON are not truncated.

Commands (`loadPanes`; each runs through `Cpd.json`, concurrently, once per call):

| Pane | cpd commands |
|---|---|
| tree | `["context","list"]`, `["plan","list"]`, `["session","list"]`, `["inbox","list"]` |
| inbox | `["inbox","list"]` |
| plan (planId non-null) | `["plan","show","--plan",planId]`; planId null -> no command, `plan: null` |

If both `tree` and `inbox` are requested, `inbox list` runs ONCE and feeds both builders. `loadAll` = all three panes. A failing command rejects the whole call with that `WebError` (no partial update). `tree.cursor` = min of the `events_cursor` of its four replies.

Behaviours: contexts group plans and sessions (5.1); the session dot derives from `state` and open inbox items (5.1 dot rule); the plan view lists workflows with their `jobKeys`, jobs with `needs` and `status`; inbox rows carry `kind` and `actions` (5.3); malformed JSON is rejected at the boundary; a failing command yields `cpd_failed`.

#### `events.ts`: ONE follow, coalesced refresh; the cockpit service

```ts
export interface CpdEvent { readonly id: number; readonly kind: string; readonly plan: string | null; readonly context: string | null; readonly session: string | null; readonly job: string | null; readonly attempt: string | null; readonly at: string; readonly text: string }
export function parseEventLine(line: string): CpdEvent; // `cpd_malformed_json` on bad JSON or a missing id/kind
export function panesFor(event: CpdEvent, selectedPlan: { id: string; contextId: string } | null): ReadonlySet<CockpitPane>;

export const COALESCE_MS = 250; // Phase 1 choice; .4: 16
export const SAFETY_REFRESH_MS = 30_000; // Phase 1 choice (pull-request rows change with no Event, scout-cockpit §2)
export const FOLLOW_RESTART_BASE_MS = 1_000; // doubles per consecutive restart, cap 30_000 (Phase 1 choice)

export type CockpitUpdate = ({ kind: "pane" } & CockpitPaneData) | { kind: "error"; error: WireError };
export interface CockpitSnapshot { tree: ContextTreeModel | null; plan: PlanViewModel | null; inbox: InboxModel | null }

export interface CockpitService {
  start(): Promise<void>; // initial loadAll, then ONE follow
  stop(): void; // idempotent: kills the follow process, cancels every timer, no update after
  snapshot(): CockpitSnapshot;
  selectedPlanId(): string | null;
  selectPlan(planId: string | null): Promise<void>; // loads the plan pane; emits {kind:"pane", pane:"plan"}; a failure emits an error update and keeps the previous plan
  subscribe(listener: (u: CockpitUpdate) => void): Disposable;
  runAction(action: CockpitAction): Promise<ActionOutput>;
}
export function createCockpitService(deps: { readModel: ReadModel; cpd: Cpd; follow: FollowBoundary; scheduler: Scheduler }): CockpitService;
```

`panesFor` (kind lists are the `KINDS` of `tui/src/tree/kinds.rs:11-62` and `docs/commands.md:5290-5296`, `scout-cockpit §2`; kind names not written in commands.md are unverified, O16):

- **tree** pane: `context-created, context-grounded, context-record, context-renamed, context-archived, plan-created, plan-phase, plan-done, plan-renamed, plan-archived, plan-review, job-added, job-edited, job-merged, job-failed, job-retried, job-cancelled, attempt-started, attempt-retry, attempt-status, all-merged, workflow-added, workflow-edited, workflow-cancelled, workflow-started, pr-opened, pr-merged, escalation, escalation-answered, escalation-expired, review-opened, review-closed, session-registered, session-state, session-moved, session-renamed, session-archived, session-asleep, plan-deleted, context-deleted, inbox-opened, inbox-closed, inbox-claimed, inbox-snoozed, inbox-dismissed`.
- **inbox** pane: `inbox-opened, inbox-closed, inbox-claimed, inbox-snoozed, inbox-dismissed, session-state, plan-created, session-registered, session-moved` (`inbox-claimed` added: the item mark changes with it, scout-cockpit §2 note).
- **plan** pane: only when a Plan is selected and the event concerns it: `event.plan === selected.id`, or `event.context === selected.contextId`, or `event.kind === "session-moved"`.
- Any kind in none of these lists -> the empty set (no refresh). `session-titled` is in no list (scout-cockpit §2).

Service behaviours:

1. `start()` runs `readModel.loadAll(null)`, stores the snapshot, then starts `follow.start(followArgs(cursor))` exactly once, where `cursor` = `tree.cursor`. A failure of `loadAll` rejects `start()` with its `WebError` and starts no follow. A second `start()` is a no-op. **.6:** the follow child runs while the cockpit stream is not ready, and is stopped by its first `hello`.
2. Each follow line goes through `parseEventLine`; the service remembers `lastCursor = max(lastCursor, event.id)`; a malformed line emits `{kind:"error"}` and is skipped. **.5 and 13.8:** while the stream is ready, its `event` frames are the source of these lines.
3. An event whose `panesFor` is non-empty adds those panes to a pending set; if no coalescing timer is pending one is scheduled `COALESCE_MS` later (`scheduler.after`). When it fires, `readModel.loadPanes(pending, selectedPlanId)` runs ONCE for the whole burst and one `{kind:"pane"}` update per pane is emitted. A burst of N events inside the window therefore triggers one refresh (never one CLI spawn per event or per render).
4. An event kind in no list triggers no `loadPanes`.
5. A failed refresh emits one `{kind:"error", error}` and keeps the previous snapshot; the pending set is cleared.
6. A safety refresh of the `plan` pane (when a Plan is selected) runs every `SAFETY_REFRESH_MS` through the Scheduler.
7. When the follow process exits while the service runs: restart `followArgs(lastCursor)` after `FOLLOW_RESTART_BASE_MS * 2^n` (n = consecutive restarts, reset by a received line, capped 30 000). Exit code 3 (`cursor_ahead`, `docs/commands.md` §11.2 precondition; live behaviour of `events follow` not run, O18) first reloads all panes (`loadAll`) and restarts from the new `tree.cursor`.
8. `stop()` kills the follow process, disposes every timer and subscription; nothing fires afterwards. `selectPlan` stores the id and loads only the plan pane.
9. `runAction(action)` delegates to `runAction(cpd, action)` of `actions.ts`; errors propagate as `WebError`.
10. `refresh(panes)` (rules 3 and 5) always returns a promise that resolves only after the in-flight loop has drained every queued pane, so a call made while another refresh runs waits for that loop. `runAction` awaits that refresh before it resolves, so `action.result` is never sent while `snap.tree` is stale (for plan.new the tree already holds the new Plan and Lead session).
11. Monotonic pane cursors (pro-d426): each pane update, whether from a coalesced `loadPanes` burst (rule 3) or from the `runAction` refresh (rule 10), is applied to the snapshot only when it is not older than the snapshot's pane: `tree` — **Amended by 14.1 (air-n13, air-n15): the guard never applies to the tree; a tree update is applied and emitted in every case, and a lower `tree.cursor` drops nothing**; `inbox` when `u.inbox.cursor >= snap.inbox.cursor` (or `snap.inbox` is null); `plan` when `u.plan` is null, or `snap.plan` is null, or the planId differs (a different Plan is a new selection, not a stale one), or `u.plan.cursor >= snap.plan.cursor`. A null plan always applies. **Amended by 15.3 (air-d44): a stale update is never dropped silently** — a drop emits one `{kind:"error"}` update, so the cockpit error toast shows it (the pane keeps its held rows).

#### `actions.ts`

```ts
export function actionArgs(action: CockpitAction): readonly string[]; // exact args of the table in 5.4; total over the union
export function runAction(cpd: Cpd, raw: unknown): Promise<ActionOutput>; // parseCockpitAction(raw) then cpd.json(actionArgs(...)); resolves the reply without `ok`
export const WEB_UNBLOCKS = "User message from the web chat"; // 1..200 chars, one line (`--unblocks` rule, scout-cockpit §3); Phase 1 choice
export function sessionSendArgs(sessionId: string, text: string, unblocks?: string): readonly string[]; // ["session","send",sessionId,"--unblocks",unblocks ?? WEB_UNBLOCKS,"--",text]; `--` so text starting with "-" is a positional (clap default; unverified for cpd, O6)
export function sessionRetierArgs(sessionId: string, tier: string): readonly string[]; // = actionArgs({kind:"session.retier",...})
```

Behaviours: inbox snooze, dismiss, approve and claim map to the `inbox` commands of 5.4; `pr.post` maps to `["pr","post","--workflow",workflow,"--plan",planId]`; `plan.reject` and `session.retier` as in 5.4; an action error code is surfaced to the caller (`cpd_failed` with `details.cpdCode`, e.g. `item_not_found`, `item_closed`, `tier_not_allowed`); an unknown `kind` rejects with `unknown_action` and runs nothing; a known kind with a bad field rejects `invalid_message`.

### 6.4 durable-srv `web/src/server/durable/`

> **Superseded by 10.8:** this section describes the v2 dialect (protocol 2, pi-durable 1.5.0) only. Phase 2 adds the v1 dialect, `hello {protocol: 2, min_protocol: 1}`, `assertSupportedHost` for both pairs, the `Dialect` interface `{transcript, applyBase, applyIncrement}` with `createDialect(protocol)`, and `ops.ts` (`applyOps`, `createReplica`, replacing `applyImmutable`). `hello {protocol: 2, min_protocol: 2, ...}`, `SUPPORTED_PI_DURABLE`, `PROTOCOL = 2`, and `applyView`/`applyOps` as Dialect methods below are superseded by 10.8; the `cpd-chat` row of the entry table is superseded by 10.2.

Protocol facts: pi-durable **1.5.0** host protocol 2. Host side: `docs/contract.md` §6.14 (the `ToHost`/`ToClient` tables, rules 1 to 11, `ring.ts` rules 1 to 5) and §6.17 (`history`), `pi/durable-host/protocol.ts` and `pi/durable-host/ring.ts`, all read by reading, not run. Pi-durable and chord shapes: `node_modules/@earendil-works/pi-durable/dist/harness/view.d.ts` (`ConversationView`, `ConversationWindow`), `dist/harness/live.d.ts` (`LiveState`, `ToolSlot`, `CompactionStatus`), `dist/harness/transcript.d.ts` and `transcript.js` (the reference client transcript: the `before` rule, the gap rule, descending pages), `dist/types.d.ts` (`EntryRecord`), `@earendil-works/pi-ai/dist/types.d.ts` (message shapes), `@earendil-works/chord/dist/delta/index.d.ts` (the ops). The `view` and `ops` shapes were verified by a real Harness run for text and entries (`$CPD_HOME/run/pi-1-5-upgrade/explore/live-check.md`); the window past five entries, its bounds and `before` after an overflow are **unverified** (O23). Every other claim of this section is verified by reading, not run, or is **unverified** and tagged with its O-item (section 9).

**Transcript dialect (A3).** The durable backend consumes the host's `view` base frame and its `ops` frames. The host window holds the newest 50 to 100 entries (`ConversationView.entries`); older entries come from the `history` command. The backend keeps a transcript cache of every entry it has seen, so the window moving never drops an item (6.4 backend, "Cache and window").

**Host frames (`protocol.ts`, `ring.ts`).**
- `hello {protocol: 2, min_protocol: 2, client: "cpd-web"}` is answered by `welcome {protocol: 2, pi_durable: "1.5.0", host_version, session, role, epoch, client_id, cwd, limits}`. A host that cannot negotiate 2 answers `error protocol_mismatch` with `details {host: [2], client: [min_protocol, protocol]}`, then closes (§6.14 rule 1).
- `attach {req, conversation: "root", cursor?}` is answered by `attached {req, mode: "view" | "resume", epoch, from_seq, head_seq, client_id}`, then the ring frames of the attach (the base `view` frame, then every `ops` frame in `seq` order, `ring.ts` rule 2 for `mode`), then one `host_state`. `mode` is `"view"` when there is no cursor, the cursor's epoch differs, or the cursor is outside the ring; `"resume"` only when the cursor is inside the ring.
- `view {epoch, seq, view}`: the base, a `ConversationView` as JSON: `{conversation, entries, before?, docs}`. `entries` is the window in entry-id order (pi-durable `view.d.ts`, verified by reading); `before` is the id of the entry just before the window, absent when the window starts the transcript (`view.d.ts`, verified by reading); `docs` holds `pi.agent`, `pi.live`, `pi.inbox`, `pi.provider`, `pi.usage` (live-check run, verified). A later `view` frame replaces the whole value (ring rule 5, a rebase).
- `ops {epoch, seq, ops}`: the ops of one watch call, plain JSON, applied in order to the last value. Ops are tuples with array paths (chord 1.5.0 `delta/index.d.ts`, verified by reading): `['r', value]` replaces the whole value; `['s', path, value]` sets a key or an in-range index; `['d', path]` deletes a key or splices out one array item; `['a', path, string]` appends to a string; `['t', path, n]` drops n leading characters; `['p', path, start, deleteCount, items]` splices an array; `['m', path, perm]` reorders an array. Live-check verified: the entry append `['p', ['entries'], n, 0, [entry]]`, `['s', ['docs','pi.live','run'], …]` and `['d', ['docs','pi.live','run']]`, `['s', ['docs','pi.live','generation'], {attempt: 1}]`, `['s', ['docs','pi.live','generation','message'], message]`, `['a', ['docs','pi.live','generation','message','content', i, 'text'], chunk]` (chunks, not cumulative) and `['d', ['docs','pi.live','generation']]`.
- `history {req, before?, limit?}` is answered by `result {req, data: {entries, next?}}` (§6.17): `entries` are `EntryRecord`s unchanged, newest first (the host's `order: "descending"`, §6.17; verified by reading, and pi-durable's `transcript.js` reverses descending pages the same way); `next` is an opaque cursor, present when more entries older than the page exist. The first request has no `before` and returns the newest page (§6.17; **unverified** on the wire, O22).

#### `client.ts`: NDJSON protocol-2 client

```ts
export interface SocketLike {
  write(line: string): void; // already newline-terminated
  end(): void; // idempotent
  onData(handler: (chunk: string) => void): Disposable; // UTF-8 decoded chunks of ARBITRARY size/boundaries
  onClose(handler: (error: Error | null) => void): Disposable;
}
export interface SocketFactory { connect(path: string): Promise<SocketLike> } // rejects when the socket cannot be opened
export const PROTOCOL = 2; export const CLIENT_NAME = "cpd-web";

export type HostFrame =
  | { type: "welcome"; protocol: number; host_version: string; pi_durable: string; session: string; role: string; epoch: string; client_id: string; cwd: string; limits: { client_line: number } }
  | { type: "attached"; req: string; mode: "view" | "resume"; epoch: string; from_seq: number; head_seq: number; client_id: string }
  | { type: "view"; epoch: string; seq: number; view: unknown } // the base, or a rebase: a ConversationView as JSON
  | { type: "ops"; epoch: string; seq: number; ops: unknown[] } // the ops of one watch call, plain JSON
  | { type: "result"; req: string; data: Record<string, unknown> }
  | { type: "error"; req?: string; code: string; message: string; details?: unknown }
  | { type: "host_state"; state: "idle" | "working" | "waiting" | "blocked"; name?: string; clients: number; held_ui: number; agent?: { model?: { provider: string; model_id: string }; thinking_level?: string | null } }
  | { type: "ui_request"; id: string; purpose: string; created_at: number; expires_at?: number | null; tool_call_id?: string; replayed?: boolean; method: string; [field: string]: unknown }
  | { type: "ui_resolved"; id: string; outcome: DialogOutcome; by?: string }
  | { type: "notice"; level: "info" | "warning" | "error"; message: string }
  | { type: "status"; key: string; text: string | null }
  | { type: "pong"; req?: string }
  | { type: "bye"; reason: "shutdown" | "restart" | "storage_failed" | "slow_client" | "detached"; reconnect: boolean };
export const FRAME_TYPES: ReadonlySet<string> = new Set(["welcome", "attached", "view", "ops", "result", "error", "host_state", "ui_request", "ui_resolved", "notice", "status", "pong", "bye"]); // the HostFrame types; `snapshot` and `events` are not among them
export function parseHostFrame(line: string): HostFrame; // JSON.parse + discriminator check against FRAME_TYPES; failure -> WebError("protocol_error", ..., {line: first 200 chars})

export interface HostCursor { epoch: string; seq: number } // host cursor (distinct from the domain `Cursor`); seq counts view and ops frames (ring.ts rule 1)
export interface HostClient {
  readonly welcome: Extract<HostFrame, { type: "welcome" }>;
  attach(cursor?: HostCursor): Promise<Extract<HostFrame, { type: "attached" }>>; // sends attach {req, conversation: "root", cursor?}; the view and ops frames that follow arrive through onFrame
  submit(input: { text: string; whenBusy: "followUp" | "steer"; requestId?: string; images?: { mimeType: ImageMime; dataBase64: string }[] }): Promise<{ submissionId: number; status: string }>; // no origin field on this submit frame (11.9): the durable host's web submissions are `cpd-chat` entries, not `session-message` Events
  compact(instructions?: string): Promise<{ taskId: number }>;
  configure(change: { model: string } | { thinkingLevel: string | null }): Promise<Record<string, unknown>>; // wire: model | thinking_level
  listModels(): Promise<{ provider: string; model_id: string; name: string; thinking_levels: string[]; context_window: number }[]>;
  history(before?: string, limit?: number): Promise<{ entries: readonly unknown[]; next?: string }>; // sends history {req, before?, limit?}; `before` is the `next` of an earlier page, opaque; `limit` is passed as given (the host defaults to HISTORY_LIMIT 50 and caps at HISTORY_MAX 500)
  abort(): Promise<void>; // abort {background:false}
  abortSubmission(submissionId: number): Promise<{ outcome: unknown }>;
  respond(id: string, answer: { value: string } | { confirmed: boolean } | { answer: object } | { cancelled: true }): Promise<void>;
  detach(): Promise<void>;
  onFrame(handler: (frame: HostFrame) => void): Disposable; // live frames: view, ops, host_state, ui_request, ui_resolved, notice, status, bye
  onClose(handler: (error: Error | null) => void): Disposable;
  close(): void; // idempotent
}
export function connectHost(socketPath: string, deps: { factory: SocketFactory; scheduler: Scheduler }): Promise<HostClient>;
```

Wire shapes cited: client frames `protocol.ts` `ToHost` (verified by reading): `hello {protocol, min_protocol, client, name?}`, `attach {req, cursor?, conversation?}`, `submit {req, text, when_busy?, request_id?, images?: [{mime_type, data}]}`, `compact {req, instructions?}`, `configure {req, model? | thinking_level?}` (exactly one), `list_models {req}`, `history {req, before?, limit?}`, `abort {req, background}`, `abort_submission {req, submission_id}`, `ui_response {req, id, value|confirmed|answer|cancelled}`, `detach {req}`; `req` is a client-chosen string and replies `result`/`error` carry it. Result `data` shapes (`protocol.ts`, §6.17, verified by reading): `submit` `{submission_id, status}`, `compact` `{task_id}`, `configure` `{agent}`, `list_models` `{models}`, `history` `{entries, next?}`, `abort_submission` `{outcome}`.

Behaviours:

- `connectHost` opens the socket through `factory.connect` (rejection → `host_unreachable`), writes `{"type":"hello","protocol":2,"min_protocol":2,"client":"cpd-web"}\n`, and waits for `welcome`. No `welcome` within 5000 ms of the Scheduler (`LIMITS.helloMs`, `protocol.ts`) → `host_unreachable`. An `error` frame before `welcome`: code `protocol_mismatch` → `unsupported_host_version` with `details {host, client}` from the frame's `details` (the host's rule 1 answer, verified by reading); any other code → `host_error` with `details {hostCode}`. A `welcome` is checked by `assertSupportedHost` (6.4 frames), so `pi_durable !== "1.5.0"` or `protocol !== 2` → `unsupported_host_version` with `details {piDurable, protocol}`. (Phase 1 choice: the handshake mapping of `protocol_mismatch`.)
- Frames are split on `\n`; a frame may be split across any number of chunks, and several frames may arrive in one chunk; a trailing partial line is buffered. An empty line is ignored. A line failing `parseHostFrame` → the client emits close with a `protocol_error` and ends the socket. `snapshot` and `events` are not frame types: a line of either type is a `protocol_error`.
- Every command gets a fresh `req` (`r<n>`); the returned promise settles on the `result`/`error` with that `req`: `error` → `WebError("host_error", message, {hostCode: code, details})`. Frames with no matching pending request are delivered to `onFrame`.
- `attach(cursor)` sends the cursor when given. The `attached` frame is returned; the `view` and `ops` frames that follow arrive through `onFrame`. The backend sends no cursor (6.4 backend).
- `submit` carries `when_busy` and `images` as `{mime_type, data}` (images omitted when none) and `request_id` when given.
- `history(before, limit)` sends `history {req, before?, limit?}` (fields omitted when undefined) and settles on the `result` with `data` `{entries, next?}`.
- `abort` sends `{"type":"abort","req":..,"background":false}`; `abortSubmission` sends `abort_submission {submission_id}`.
- Socket close (or `bye`) while the client is open: every pending request rejects `host_disconnected`, `onClose` fires once with `host_disconnected`. `close()` ends the socket and rejects pending requests with `host_disconnected`; after it nothing is emitted.

#### `frames.ts`: the single dialect seam (pi-durable 1.5.0, protocol 2)

Every `view` and `ops` frame reaches the dialect, and nothing else touches the replica or the cache.

```ts
export interface DurableTranscript { readonly items: TranscriptItem[]; readonly facts: DurableFacts; readonly before?: number /* view.before of the replica: the id just before its window; absent when the window starts the transcript */ }
export interface DurableFacts {
  model?: string; // provider/modelId, from docs["pi.agent"].model
  thinking?: string | null; // docs["pi.agent"].thinkingLevel
  busy: boolean; // docs["pi.live"].run !== undefined
  costUsd: number; // see "Facts"
  lastContextTokens?: number; // see "Facts"
  queued: { id: number; mode: string }[]; // docs["pi.inbox"].items without mode "write"
  cwd?: string; // welcome.cwd, set by the backend
}
export interface Gap { readonly after: number; readonly upTo: number } // entry ids in (after, upTo] are not held
export const SUPPORTED_PI_DURABLE = "1.5.0";
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void; // else WebError("unsupported_host_version", msg, {piDurable, protocol}) unless pi_durable === SUPPORTED_PI_DURABLE and protocol === 2
export interface Dialect {
  applyView(frame: { view: unknown }): { transcript: DurableTranscript }; // the base (first call) or a replacement
  applyOps(frame: { ops: unknown[] }): DialectOutput;
  applyHistory(frame: { entries: unknown[]; next?: string }): void; // one history page
  readonly gap: Gap | undefined;
  readonly transcript: DurableTranscript; // the cache and the live tail (below)
}
export interface DialectOutput {
  append: TranscriptItem[];
  update: TranscriptItem[];
  deltas: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }[];
  busy?: boolean;
  queued?: { id: number; mode: string }[];
  agent?: { model?: string; thinking?: string | null };
  replaced?: boolean; // the transcript was rebuilt: the backend sends a new tail window and ignores append, update and deltas
}
export function createDialect(): Dialect;
export function dialogFromRequest(req: Extract<HostFrame, { type: "ui_request" }>, transcript: readonly TranscriptItem[]): Dialog | { kind: "editor"; id: string } | { kind: "unsupported"; id: string; method: string; purpose: string }; // 3.8 table
```

**The cache.** Each entry is normalised once, when it is first seen (by its `id`, in a `view`, an `ops` batch or a history page), and kept for the attachment. A window moving past an entry never removes it. Entries never change once committed (pi-durable `transcript.js`, verified by reading), so a cached entry is never normalised again. `transcript.items` is the items of the cached entries in id order, followed by the live tail (below).

**Replacement and gap.** `applyView` (every call) and an ops batch whose first op is `['r', …]` replace the replica. Let `n` be the newest cached entry id before the new value's entries are merged (undefined for an empty cache). If the new value's `before` is defined, `n` is defined and `before > n`, then `gap = {after: n, upTo: before}`: the entries with ids in (`n`, `before`] were never seen (the rule of pi-durable's `ConversationTranscript`, `transcript.js` `#cover`, verified by reading). `applyHistory` clears the gap when the page's oldest entry id is at most `after`, or when the page has no `next`. The backend fills an open gap before it shows anything (6.4 backend).

**`applyView`.** Replaces the replica with the view, merges its entries (the cache rule), and recomputes the live tail (below). It emits no output: `transcript.facts` carries the same busy, queued, model and thinking values that a batch's `DialectOutput` carries. Throws `protocol_error` when the value is not an object with an `entries` array and a `docs` object.

**`applyOps`.** In this order:
1. No view applied yet: `protocol_error` ("ops before view").
2. `next = applyImmutable(replica, ops)` (`@earendil-works/chord/delta`, copy on write). A thrown op is `protocol_error` carrying the applier's message; the replica is unchanged.
3. `next` must be an object with an `entries` array and a `docs` object, else `protocol_error`.
4. `replaced` is true when `ops[0]` is `['r', …]`; the gap rule applies to `next` as above.
5. Entries of `next` not yet cached are merged in id order (a pi.assistant entry is matched to the live item here, rule L4). Entries are merged before step 6, so a live item is consumed in the batch that appends its entry.
6. The live tail is recomputed from `next.docs['pi.live']` (rules L1 to L5).
7. The output is produced (table below); the replica becomes `next`.

**Output (`DialectOutput`).** When `replaced` is true, `append`, `update` and `deltas` are empty: the cache and the live tail are updated, and the backend sends the tail window. Otherwise:

| Field | Source and rule |
|---|---|
| `append` | Items of entries newly cached in this batch, in id order, except those that take the place of a live item (`update`). Then the live assistant item when it is created (L2). Then tool items created by a toolCall block (L2) or a new tool slot (L5), in block or slot order. An item whose id is already in the transcript never goes to `append`. |
| `update` | The entry item that takes the place of a live item (L4, keeps the live id). A live item whose message changes by any op other than `'a'` on its content (its blocks are replaced whole), or that is finalised (L3). A tool item whose slot status changed (L5), whose arguments were refreshed while it has no result (L2), or that received its result by pairing (T below). Cached items changed by pairing. |
| `deltas` | One per `['a', path, chunk]` op whose path is `['docs','pi.live','generation','message','content', i, 'text']` or `[…, i, 'thinking']`, with `blockIndex` = the number of text and thinking blocks at content indices below `i` (the index among the assistant item's `blocks`, which exclude toolCall blocks), `blockType` by the last path segment, `text` = the chunk. Emitted only when no other op of the batch changes the message (otherwise the whole item is in `update`). Argument-path appends produce no delta (L2, O21). |
| `busy` | `docs['pi.live'].run !== undefined`; set when it changed. |
| `queued` | `docs['pi.inbox'].items` with `mode` `steer` or `followUp`, as `{id, mode}`; an item with `mode` `write` is not a queued submission and is filtered out. Set when `docs['pi.inbox']` changed. |
| `agent` | `docs['pi.agent']`: `model` = `provider + "/" + modelId` when `model` is present; `thinking` = `thinkingLevel` when present. Set when `docs['pi.agent']` changed. |
| `replaced` | true for an ops batch whose first op is `['r', …]`. A `view` frame after the first is the same case: `applyView` emits nothing and the backend treats it as `replaced: true`. |

**Live tail (`docs['pi.live']`; `live.d.ts` `LiveState`, verified by reading).** The live tail is the list of items that are not yet entries. It is recomputed from each new value; a live item is identified by its id `live:<n>`, `n` counting from 1 per attachment.

- **L1 busy.** `busy` is `docs['pi.live'].run !== undefined` (live-check run: present while a run is open, absent after its end, verified).
- **L2 message.** `M = docs['pi.live'].generation?.message` (AssistantMessage JSON). When `M` holds a `text` or `thinking` block, or a non-empty string `M.errorMessage`, and no live item exists, a live assistant item is created: id `live:<n>`, `streaming: true`, `blocks` = the `text` and `thinking` blocks of `M.content` in order (`text` block: its `text`; `thinking` block: its `thinking`), then, when `M.errorMessage` is a non-empty string, the text block `"Error: " + M.errorMessage`; appended at the end of the tail. A message with neither yields no live assistant item (as a `pi.assistant` entry does). Each `toolCall` block of `M.content` yields a running ToolItem `tool:<block.id>` (`name`, `args` = `block.arguments` when it is an object, else `{}`), created in `append`; its `args` is refreshed in `update` only while the item has no result. Argument appends (`arguments` or `partialJson` paths) are not read: the complete arguments arrive with the entry (**unverified**, O21).
- **L3 finalise.** When `M` is absent and a live item exists, the item is finalised: `streaming: false`, `update`, and it leaves the live state (it stays in the tail as a plain item). This happens after L4 in the same batch.
- **L4 entry replaces live.** A `pi.assistant` entry merged in this batch (step 5) takes the place of the live assistant item when one exists: its assistant item keeps the live id (`live:<n>`, not `a:<id>`), `streaming: false`, and goes to `update`; the entry's tool items take the ids `tool:<callId>` of the live tool items they match (`update` when present, else `append`). The live state is then cleared. Without a live item, the entry is normalised as an ordinary `pi.assistant` entry (`a:<id>`, `append`).
- **L5 tool slots.** `docs['pi.live'].tools` (absent: `[]`) is read per slot by `callId`; a slot `status` outside `pending|running|done` is `protocol_error`.
  - A slot with no tool item yet creates one: `tool:<callId>`, `name` = the slot's `name`, `args` `{}` unless a toolCall block gave arguments, status `running`, `append`.
  - `pending` and `running` slots keep the item `running`. Their `output`, `droppedBytes`, `droppedLines` and `details` are not shown: a running tool has an empty body (6.8, ToolRow).
  - A `done` slot with an `entry` completes the item with that `pi.tool-result` entry (rule T), when that entry is cached.
  - A `done` slot without an `entry` makes the item `failed` with `result {text: "", isError: true}`, `update`.
  - `unoffered` is not special-cased. A slot is `update`d when its status changed since the last value.
- **L6 compaction failure.** `docs['pi.live'].compactionFailure`, when present, is a `FoldItem` at the end of the live tail: `id` `cf:<taskId>`, `foldKind` `"cpd"` (an existing fold kind; no new kind), `title` `Compaction failed (<trigger>)`, `body` = `error`, `defaultOpen` true. `taskId`, `trigger` and `error` must be strings (pi-durable 1.5.0 `live.d.ts:69-75`; the values of `CompactionReason` are not read, **unverified**); any other shape is `protocol_error`. It is recomputed from each value, so it leaves the tail when a later value has no `compactionFailure` (a compaction that produces a summary removes it, `live.d.ts:161-164`).
- Other live documents (`nestedTools`, `compactions`, `runFailure`, `retry`, `deferred`) are not mapped to items: no item kind exists for them.

**Tool results (T).** A `pi.tool-result` entry pairs with the item of the same `callId` (`model[0].toolCallId`) wherever it is: in the cache or in the live tail. The item takes `result {text, isError, durationMs?}` and `status` `done` (isError false) or `failed` (isError true), and `diff` for `name === "edit"` (below). Pairing works in either order: a call normalised after its result takes the result at normalisation. A result whose call is in no cached item or live item is an `OrphanResultItem` (`orphan:<callId>`); when its call is normalised later, the orphan is removed from the cache and the call takes the result (the browser's adoption, 4.4, does the same for the window).

**Facts (`DurableFacts`).**
- `costUsd` = the sum of `cost.total` over `docs['pi.usage'].models` (every value of the record; a value without a numeric `cost.total` is `protocol_error`). A compaction does not reset it: the cost of turns a compaction removed from the transcript stays in the sum, because `pi.usage` is cumulative per model (live-check run, verified for the model record's growth over two runs; compaction not run, **unverified** O24). This is the change from the replaced protocol-1 footer, which summed the active `pi.assistant` entries and reset at a compaction.
- `lastContextTokens` = `usage.totalTokens` of `docs['pi.live'].generation.message` while a message is streaming and that value is a number; else `usage.totalTokens` of the `pi.assistant` entry with the highest id in the cache; absent when neither exists.
- `model`, `thinking` from `docs['pi.agent']` (as `DialectOutput.agent`); `busy` from L1; `queued` from `docs['pi.inbox']`; `cwd` from `welcome.cwd` (set by the backend).

**Entry normalisation (applied to `entries` of every `view`, `ops` batch and history page).** By `EntryRecord.kind`:

| kind | Result |
|---|---|
| `pi.user` | `UserItem` (`u:<id>`) from `model[0]` (`UserMessage`: `content` string, or `TextContent`/`ImageContent` blocks; text blocks joined with `"\n"`, image blocks → `TranscriptAttachment {kind:"image", mimeType, dataBase64}`) |
| `pi.assistant` | `AssistantItem` (`a:<id>`, `streaming:false`, or the live id under L4) from `model[0]` (`AssistantMessage`): `text` → text block, `thinking` → thinking block (kept separate, in order), a non-empty string `errorMessage` → a final text block `"Error: " + errorMessage` (pro-d435), `toolCall` → a `ToolItem` (`tool:<callId>`, `args = arguments`, `status:"running"`, unless the item already has a result) appended right after the assistant item; `model` → `AssistantItem.model`. `usage` is read by the facts rules, not here. |
| `pi.tool-result` | pairs by `callId` (rule T): `result {text, isError, durationMs}`, `status`, and `diff` from `details.patch ?? details.diff` for `name === "edit"` (strings only; O3) |
| `pi.compaction` | `CompactionItem` (`c:<id>`), `summary` = text of `model[0]` |
| `pi.system`, `pi.reset` | dropped |
| `cpd-context`, `cpd-plan-summary`, `cpd-plan-decision`, `cpd-skills` (host-written, `model: [{role:"user", content}]`, `data` = details; `pi/durable-host/plan-review.ts:102-113`, `turn.ts:88`, read) | `FoldItem` (`f:<id>`), `body` = text of `content`, title and `defaultOpen` by the rules of 3.3 (`data` plays `details`) |
| `cpd-chat`, `cpd-event`, `cpd-first-message`, `cpd-error`, `cpd-plan-review`, any other kind | dropped |

An entry whose `kind` is a `pi.*` kind but whose `model` is missing, empty or not a message of the expected role → `protocol_error`. An `EntryRecord` without `id` or `kind` → `protocol_error`. Whether `cpd-context` entries carry `data.summary` in the durable path is unverified (O14); the title then falls back to the first body line.

**Errors (`protocol_error`, every case).** Each raised as `WebError("protocol_error", message)`:
- an `ops` frame before any `view` (`ops before view`);
- an op the applier refuses (`applyImmutable` throws), with the applier's message;
- a value (base, replacement or ops result) that is not an object with an `entries` array and a `docs` object;
- an unknown entry shape (the `pi.*` and missing-field cases above, including a history entry);
- a tool slot `status` outside `pending|running|done`;
- a `docs['pi.usage']` model value without a numeric `cost.total`.

A `protocol_error` from any of these ends the attachment (`backend.ts`, Failure).

**Behaviours to test.** A `view` base normalises its entries and live tail with no output; an `ops` batch appends an entry (`p`) and its items; `run` presence drives `busy`; a text `a` op yields one delta at the text block's index, with thinking blocks counted; a message with only a toolCall yields a running tool item and no assistant item; a pi.assistant entry takes the live id and goes to `update`, and a later entry with no live item is appended as `a:<id>`; a done slot with an entry completes its tool item by pairing; a done slot without an entry becomes failed; a result without its call is an orphan and its call adopts it; `['r', …]` and a second `view` are `replaced`; a `before` above the newest cached id opens a gap that `applyHistory` clears; the cost is the sum over `pi.usage` models; an ops batch before any view, an op the applier refuses, an unknown entry shape and a bad slot status each raise `protocol_error`; `assertSupportedHost({pi_durable:"1.3.0", protocol:2})` and `({pi_durable:"1.5.0", protocol:1})` raise `unsupported_host_version`.

#### `backend.ts`: `SessionBackend` for durable sessions

```ts
// SessionBackend, SessionHandle, SessionEmit, ResolvedSession and GitBoundary are the shared interfaces of 6.1 (runtime.ts)
export function createDurableBackend(deps: { connect: (socketPath: string) => Promise<HostClient>; cpd: Cpd; git: GitBoundary; scheduler: Scheduler; windowSize?: number }): SessionBackend;
```

**Attach retry.** `attach` calls `deps.connect(socketPath)` once per attempt, so every attempt opens a fresh socket. A fresh Lead has no host socket until `cpd watch` spawns the host on its next tick, so `connect` rejects `host_unreachable` for a while. Only a `host_unreachable` rejection of `deps.connect` is retried; a missing socket path is refused before any connect and is not retried. The first attempt runs at once and the backend records `t0 = scheduler.now()` before it. After a failed attempt at time `t`: if `t - t0 < 15000` ms, the next attempt runs `deps.scheduler.after(250, …)` later; otherwise `attach` rejects with that last `host_unreachable` error and makes no further attempt. Any other rejection of `deps.connect` rejects `attach` at once. The retry stops when the optional `attach` `AbortSignal` aborts: the pending timer is cancelled, no connect follows, and `attach` rejects `host_unreachable` `{reason: "aborted"}`. A rejection of `attach` is emitted as the attach's `session.error` (the hub's `session.attach` rule of 6.6), which 6.2 Retry and the status line of 6.11 show. **Open:** 6.1 gives a pending `attach` no handle, so how a detach that arrives during the window reaches that timer is not defined here (**unverified**: no code exists yet).

**Frame order.** Every host frame of an attachment (`view`, `ops`, `host_state`, `ui_request`, `ui_resolved`) and every `loadEarlier` call is processed one at a time, in arrival order, by one serial queue. A job that needs a `history` call (a gap fill, a loadEarlier page) waits for that call; the jobs arriving meanwhile queue behind it. Nothing is emitted out of order.

**First-frame gate.** The attach sends no cursor. `attached.mode` must be `"view"`; `"resume"` is `protocol_error` (no cursor was sent). The first frame after `attached` must be a `view`: an `ops` frame or any other data frame before it is `protocol_error`. The first `view` is applied as soon as it arrives; the frames after it wait until the attach messages below are out, then replay in order.

**Cache and window.** The transcript is `dialect.transcript.items` (6.4 frames, the cache and the live tail). The backend adds two pieces of state per attachment:
- `older`: `undefined` until a history page is needed, a page cursor (opaque `next`) while more history exists, `null` once the host has no older entries. At the base `view`, `older` is `null` when `transcript.before` is absent (the window starts the transcript) and `undefined` otherwise.
- the gap of `dialect.gap`, which the backend fills (below).

A window is a slice of `transcript.items`:
- **Cursor.** `session.window` cursors are `dur:<id>`, where `<id>` is the item id of the first item of the window (3.4). `dur:` with an empty id names the end of the cache (no item; used when the cache holds no item). A cursor is `dur:<id>` only: an id that is not an item of the transcript (cache or live tail) is `invalid_message`.
- **Tail** (`mode "tail"`): the last `windowSize` (default `WINDOW_SIZE`) items of the transcript. `hasEarlier` = the tail does not start at index 0, or `older !== null`. `cursor` = `dur:<id of the first item>` when `hasEarlier` (the empty id when the tail holds no item), else `null`.
- **Earlier** (`loadEarlier("dur:<id>")`): let `p` be the number of items before the cursor: the index of the cursor's item, or `items.length` for the empty id. While `p < windowSize` and `older !== null`, fetch the next history page (`client.history(older ?? undefined)`, which for `older === undefined` is the newest page), `dialect.applyHistory(page)`, and set `older` to the page's `next` or `null` when there is none; recompute `p` (the item keeps its id, so the index moves with the prepends). The page is `items.slice(max(0, p - windowSize), p)`. Emit `session.window {mode:"earlier"}` with `hasEarlier` and `cursor` computed as in the tail. Items past the cursor are never repeated.
- An entry already cached is skipped by `applyHistory`, so a page that holds only known entries is not an error: the loop goes on until `p >= windowSize` or `older === null`. The page is sliced after the loop, so an empty cache is filled from history alone.

**Gap fill.** After a replacement (a `view` after the first, or an ops batch with `replaced: true`) whose `dialect.gap` is defined, the backend fills the gap before it emits anything for that frame: it calls `client.history(undefined)` (the newest page), `dialect.applyHistory(page)`, and repeats with the page's `next` while `dialect.gap` is still defined. Then it emits the replacement. No `session.append`, `session.update` or `session.delta` is sent before the fill is complete.

**Attach emission.** `session.attached` (capabilities of section 8, `title = ref.title`, `meta {ref, footer}`), `session.window {mode:"tail"}`, `session.footer`, `session.status`, then one `session.dialog` per replayed held `ui_request` (the host replays them with `replayed: true` after the frames, §6.14 and `protocol.ts`).

**Live.** Each `ops` frame goes through `dialect.applyOps` (6.4 frames). If `output.replaced`, the gap rule runs and the backend emits `session.window {mode:"tail"}`; otherwise it emits `session.append` (`append`), `session.update` (`update`) and `session.delta` for each delta in order. Then `session.footer` and `session.status` as below. A `view` after the first runs the gap rule and emits `session.window {mode:"tail"}`, then the footer and status.

**Status, host state and agent.** `busy` and `queued` come from `transcript.facts` (`busy`, `queued`), as in every frame. `session.status {busy, queued}` is emitted when either changed. `host_state` frames update the agent only (`agent.model` as `provider/model_id`, `agent.thinking_level`); busy is not read from `host_state.state` (the run presence of `docs['pi.live']` carries it; **unverified** while a dialog is held, O25).

**Submit.** `text.trim() === ""` with no images → `invalid_submission`. Otherwise `client.submit({text, whenBusy, requestId: clientSubmissionId, images})`, then `session.submitAck {clientSubmissionId, queued: busy, submissionId}`. When busy, the submission is added to `queued` (`{clientSubmissionId, submissionId, text, whenBusy}`) and removed when `transcript.facts.queued` no longer holds its `submissionId`; `session.status` is emitted on each change. A host `error` becomes `session.error {clientSubmissionId, error: host_error}` and the promise rejects with it.

**Other methods.** `abort` → `client.abort()`. `abortSubmission(clientSubmissionId)` looks up the `submissionId` of that queued submission (unknown → `invalid_message`), sends `abort_submission`, and removes the entry. `compact(instructions)` → `client.compact`. `listModels` → `client.listModels()` mapped to `ModelChoice[]` (`model = provider + "/" + model_id`) → `session.models`. `configure`: `{kind:"model"}` → `client.configure({model})`; `{kind:"thinking"}` → `client.configure({thinkingLevel})`; `{kind:"tier"}` → `cpd session retier <id> --tier <tier>` via `deps.cpd` (`sessionRetierArgs`), which sets `pendingTier` and changes no footer Tier (**The applied Tier**, below). A host refusal `refused` (a run is pending, §6.17) surfaces as `host_error` with `details.hostCode: "refused"`. After a model or thinking change the footer is recomputed and `session.footer` is emitted. `respond` maps per the table of 3.8 (dialog handling: `ui_request` and `ui_resolved` frames, `dialogFromRequest` over the cached transcript).

**Footer.** `model` = `host_state.agent` model (`provider/model_id`) when given, else `transcript.facts.model`; `thinking` likewise from the agent's `thinking_level`, else `transcript.facts.thinking`; `tier` = `appliedTier` (**The applied Tier**, below); `cost` = `transcript.facts.costUsd` (the cumulative `pi.usage` cost, so compacted turns stay in it); `contextPercent` = `round(lastContextTokens / contextWindow * 100)` where `contextWindow` is `context_window` of the current model in `list_models` (fetched once at attach; an approximation, not pi-core's formula, O8); `branch` = `git.branch(cwd)` with `cwd` from `welcome.cwd`. A missing input gives `{state:"unknown", reason}`.

**The applied Tier.** `appliedTier` is per attachment and starts as `ref.tier` at attach. The footer shows the Tier the host has applied (User decision D2, pro-d430). A `configure {kind:"tier"}` runs `cpd session retier` (the row's target, pro-d86) and sets `pendingTier` only: it sets no `appliedTier` and emits no footer change for the Tier. A newer request replaces an older `pendingTier`. While `pendingTier` is set:
1. A `pi.agent` change in an `ops` frame (the host's applied change, docs/contract.md §6.18) promotes it at once: `appliedTier = pendingTier`, `pendingTier = null`, and `session.footer` is emitted. No `session show` is read for this, because a busy host configures nothing.
2. `cpd session show <id>` (`deps.cpd.json`) is read only when `session.status` goes from busy to idle while `pendingTier` is set and no `pi.agent` change has arrived since the request. If its `switch` has `tier` equal to `pendingTier`, the request failed (`host_busy`, commands §10.16): `pendingTier = null`, `appliedTier` is kept, and `session.footer` is emitted. A `switch` of null or of another Tier keeps `pendingTier`.

**Residuals (open, not decided here).** An unrelated `pi.agent` change while a request is pending promotes it early. A request the host never answers stays pending until a failure is read at a busy-to-idle transition or an agent change arrives. A failure recorded before attach is not seen at attach, so `appliedTier` starts at `ref.tier` (unverified).

**Dialogs.** A `ui_request` becomes `session.dialog` via `dialogFromRequest`, a `ui_resolved` becomes `session.dialogResolved`, `respond` answers per 3.8. The plan-review title and summary are read from the cached transcript.

**Failure.** A `protocol_error` from `applyView`, `applyOps` or `applyHistory` is sent as `session.error {sessionId, error}`, and the attachment ends: the handle rejects every method with that error until disposed, and the client is closed (`client.detach()` best effort). A replica that missed a batch cannot be trusted, so the attachment does not continue (Phase 1 choice, fail loud). Socket close (`onClose`/`bye`) → `session.error {sessionId, error: host_disconnected}`; the handle then rejects every method with `host_disconnected` until disposed. `dispose()` detaches (`client.detach()` best effort), closes the client and drops every listener.

**Behaviours to test.** `attached.mode` `resume` and an `ops` frame before the first `view` each raise `protocol_error`; the attach emits in the order above; a tail window holds the last `windowSize` items with `dur:<id>`; `loadEarlier` answers from the cache first and then from `history` pages (already-held entries skipped), with the cursor of the page; a gap is filled by `history` before the replacement window is emitted, and no append precedes it; a `replaced` ops batch emits a tail window and no append; a rejected `ops` batch ends the attachment with `session.error`; `submit` queues while busy and drops the queued entry when the inbox no longer holds it; `host_state` changes the agent but not busy; the cost and context facts follow 6.4 frames; `configure` tier runs `cpd session retier` and the applied-Tier rule above holds (promotion on a `pi.agent` change; a failed `switch` of the pending Tier is read at busy-to-idle and keeps the footer).

### 6.5 pi-srv `web/src/server/pi/`

> **Superseded by 10.1:** the `custom_message` row and `countItems` of `jsonl.ts` below: `customType` `cpd` is normalised (User item or `cpd` fold) instead of dropped.

Facts: scout-pi §1-§6 (sample file of 24 lines, source reads), `cpd session show` fields (scout-cockpit §1.1). A pi session is a JSONL file, one JSON object per line, each `{"type": ..., ...}`.

#### `jsonl.ts`: line normalisation

```ts
export interface LinePosition { readonly line?: number; readonly offset: number } // line: 1-based when known (whole-text reads), absent in windowed/appended reads; offset: byte offset of the line start
export interface JsonlEntry { readonly type: string; readonly id?: string; readonly timestamp?: string; readonly [k: string]: unknown }
export function parseLine(text: string, at: LinePosition): JsonlEntry; // WebError("malformed_line", msg, {line?, offset}) when not valid JSON, not an object, or `type` is not a string

export interface JsonlFacts { cwd?: string; sessionId?: string; name?: string; model?: string /* provider/modelId */; thinking?: string; lastStopReason?: string; lastRole?: "user" | "assistant" | "toolResult" }
export interface PushResult { added: TranscriptItem[]; updated: TranscriptItem[]; dialogsOpened: Dialog[]; dialogsResolved: string[]; factsChanged: boolean }
export interface PiNormalizer { push(text: string, at: LinePosition): PushResult; readonly facts: JsonlFacts }
export function createNormalizer(): PiNormalizer; // stateful: remembers tool calls (by callId), the last cpd_plan_review title, the cpd-plan-summary bodies by review id (`details.review`), pending ask_user_question calls and pending plan reviews
export function normalizeText(text: string): { items: TranscriptItem[]; facts: JsonlFacts; openDialogs: Dialog[] }; // whole text, "\n"-separated lines numbered from 1; blank lines are skipped; applies `updated` into the item list by id
export function countItems(entry: JsonlEntry): number; // items the entry adds in isolation: user/assistant (one item when it has a text or thinking block, or a non-empty string `errorMessage`, even when its content holds no non-empty text or thinking block; see pro-d435)/tool calls/fold/compaction; toolResult, system, header and bookkeeping lines count 0
```

Line mapping (`type` of the entry; sample `scout-pi §1`):

| `type` | Result |
|---|---|
| `session` | `facts.cwd`, `facts.sessionId` |
| `session_info` | `facts.name` |
| `model_change` | `facts.model = provider + "/" + modelId`; `factsChanged` |
| `thinking_level_change` | `facts.thinking = thinkingLevel`; `factsChanged` |
| `message` role `system` | ignored (not a chat turn, scout-pi §1) |
| `message` role `user` | `UserItem` `u:<id>` (`content` blocks as in 6.4: text joined `"\n"`, image blocks -> attachments; `[Attached image: <path>]` lines are split out, 11.3); `at` = `timestamp` (a number is converted to ISO) or the message's own `timestamp` |
| `message` role `assistant` | `AssistantItem` `a:<id>` when it has a `text`/`thinking` block or a non-empty string `errorMessage` (blocks in order, empty `thinking` dropped, the `Error: <errorMessage>` text block last; `model`), then one `ToolItem` `tool:<toolCall.id>` per `toolCall` block (`args = arguments`, `status:"running"`) in `added`; `facts.lastRole/lastStopReason` |
| `message` role `toolResult` | the `ToolItem` with `callId === toolCallId`: replaced with `result {text, isError, durationMs}`, `status`, and (`name === "edit"`) `diff = details.patch ?? details.diff` (strings only), listed in `updated`; when no call was seen by this normalizer -> `OrphanResultItem` `orphan:<toolCallId>` (`name = toolName`) in `added`. Resolves a pending `ask_user_question` dialog `call:<id>` (`dialogsResolved`) |
| `custom_message` | fold per 3.3: `customType` in the four fold kinds and `display !== false` -> `FoldItem` `f:<id>` (`body` = `content` string or its text blocks; `details` as in 3.3); `display === false`, `cpd-first-message`, `cpd-reminder` and every other `customType` -> dropped. Line fields unverified in a real file (O1) |
| `compaction` | `CompactionItem` `c:<id>` with `summary` (string) and `tokensBefore` (number, optional). Shape unverified (O2); a line without a string `summary` -> `malformed_line` |
| `custom` `customType: "cpd-plan-review"` | `data.status === "pending"` -> `dialogsOpened` a plan-review Dialog `review:<data.review>` (`interactive:true`, 11.4; `title` = `data.title` when the entry has it, else `args.title` of the last `cpd_plan_review` tool call seen, else `"Plan review <review>"`; `summary` = `data.summary` when the entry has it, else the body of the `cpd-plan-summary` fold whose `details.review` is this review, else `""`; never the summary of another review). The summary line arrives AFTER the pending entry in every real `.jsonl` (pi queues `sendMessage` until the turn ends, `appendEntry` writes at once), so when the `cpd-plan-summary` of a review that is still pending is folded, the normaliser emits `dialogsOpened` `review:<review>` again with that summary, replacing the open dialog; a review already resolved is not reopened. Pending reviews are kept by review id, each with its own title and summary; `data.status` in `approved|rejected|superseded|lost` -> `dialogsResolved: ["review:<review>"]` |
| `custom` other, `label`, `branch_summary`, `session`-adjacent bookkeeping, any other `type` | ignored (pi adds entry kinds; this is deliberate forward-compatibility, only the lines of this table render) |

Additionally an assistant `toolCall` named `ask_user_question` whose result has not been seen -> `dialogsOpened` `call:<callId>` (`interactive:true`, 11.4; `questions` from `arguments.questions` when they pass `parseDialog`; invalid arguments open no dialog). Edit `details.diff`/`patch` in a real file: unverified (O3). A line that parses but has the wrong field types for its `type` (for example `message` without an object `message`) -> `malformed_line`.

Behaviours: message lines normalise to user and assistant items; tool calls pair with tool results across lines; a result without its call becomes an orphan; compaction entries become compaction items; `model_change` and `thinking_level_change` update `facts`; cpd custom messages fold with the right `defaultOpen`; hidden custom messages are dropped; a malformed line throws `malformed_line` carrying the line number (`normalizeText`) or the byte offset; edit results carry a diff; dialogs reconstruct from tool calls and plan-review entries.

#### `window.ts`: bounded reads over an injected file boundary

```ts
export interface FileStat { readonly size: number; readonly identity: string } // identity changes when the file is replaced (dev:inode)
export interface FileBoundary {
  stat(path: string): Promise<FileStat>; // rejects when missing/unreadable
  read(path: string, start: number, end: number): Promise<Uint8Array>; // bytes [start, end); never called with a range larger than CHUNK_BYTES
}
export const CHUNK_BYTES = 65_536; // Phase 1 choice
export function cursorOf(offset: number): Cursor; // `pi:<offset>`
export function offsetOf(cursor: Cursor): number; // WebError("invalid_message") when not `pi:<non-negative integer>`
export interface TailWindow { window: Window; startOffset: number; endOffset: number; identity: string; normalizer: PiNormalizer; openDialogs: Dialog[] }
export function readTail(path: string, fs: FileBoundary, size?: number): Promise<TailWindow>; // size default WINDOW_SIZE
export function readEarlier(path: string, fs: FileBoundary, before: Cursor, size?: number): Promise<{ window: Window; startOffset: number }>;
```

Algorithm and rules (a test is written from these):

1. **Never the whole file.** Reads walk backwards from the end in ranges of at most `CHUNK_BYTES`; a 25 MB file with a 40-item tail performs a handful of `read` calls, none beyond what is needed.
2. **End offset.** `readTail` stats the file (`size`), finds the last `\n` (scanning backwards); `endOffset` = index after it. Bytes after `endOffset` (a partial last line) are NOT emitted until a later read finds their newline. An empty file (`size === 0`) or a file without any `\n` yields `window {items: [], hasEarlier: false, cursor: null}` with `startOffset = endOffset = 0`.
3. **Whole lines only.** Chunk boundaries never split a line: the first piece of a chunk is held and joined with the next older chunk (and with its tail, for a multi-byte UTF-8 sequence, decoding happens per complete line).
4. **Counting.** Lines are consumed newest to oldest; after each line `count += countItems(entry)`; consumption stops after the line that makes `count >= size`, or at offset 0. The window therefore holds whole lines and at least `size` items unless the start of the file is reached; it may exceed `size` by at most the items of one line.
5. **Normalising.** The consumed lines are then fed oldest first to a fresh `PiNormalizer` (`push` with `{offset}` only), applying `updated` by id: a result whose call lies outside the window is an orphan; a call whose result lies in the window is paired.
6. `startOffset` = byte offset of the first consumed line. `hasEarlier = startOffset > 0`; `cursor = cursorOf(startOffset)` or `null`.
7. `readEarlier(before)` repeats 2-6 with `endOffset := offsetOf(before)` (which is a line start, so no stat/partial handling is needed); `startOffset` of the page becomes the new cursor. A page may be empty (`items: []`, `hasEarlier: false`) when only bookkeeping lines precede (header, system prompt).
8. A line failing `parseLine` -> `malformed_line` with `details {offset}` (line number absent in windowed reads). `stat`/`read` rejection -> `session_file_unreadable`.

#### `watch.ts`: append-only live reads

```ts
export interface WatchBoundary { watch(path: string, onChange: () => void): Disposable } // fs.watch-like; the callback carries no payload
export type WatchEvent =
  | { kind: "append"; result: PushResult; endOffset: number } // result merges all complete new lines of one read
  | { kind: "reset"; reason: "truncated" | "replaced" }; // the watcher is inert afterwards
export function watchSession(deps: { path: string; fs: FileBoundary; watch: WatchBoundary; normalizer: PiNormalizer; offset: number; identity: string; onEvent: (e: WatchEvent) => void; onError: (e: WebError) => void }): Disposable;
```

Behaviours: each change notification triggers at most one read in flight (changes during a read cause exactly one more read afterwards, no timers); `stat`: identity differs -> `reset replaced`; `size < offset` -> `reset truncated`; `size === offset` -> nothing; otherwise `read(path, offset, size)` (only the appended bytes, in `CHUNK_BYTES` pieces), complete lines are parsed in order through the shared `normalizer`, `offset` advances to just after the last `\n`; a partial last line waits for its newline; an append produces `added`/`updated` (the backend turns them into `session.append` / `session.update`). `stat`/`read` failures -> `onError(session_file_unreadable)` and the next change retries. `dispose()` disposes the boundary watch handle, and results of an in-flight read are discarded.

#### `backend.ts`: `SessionBackend` for pi sessions

```ts
export function createPiBackend(deps: { cpd: Cpd; fs: FileBoundary; watch: WatchBoundary; git: GitBoundary; cpdHome: string; attachmentsDir: string; now(): number; scheduler: Scheduler; windowSize?: number }): SessionBackend;
```

Behaviours:

- `attach(session)`: `session.file` missing (pro-d397, pro-d400: a by-user pi session writes its transcript only with its first reply) -> emits `session.attached` (full capabilities, `title = ref.title`), `session.window {mode:"tail", items: [], hasEarlier: false, cursor: null}` and `session.status` with `busy` false, and starts no watch. Otherwise `readTail(file)`, then `cpd session show <id>` (`deps.cpd.json(["session","show",id])`, fields `session.model`, `session.thinking`, `session.tier`; a failure does not fail the attach: those stats are `unknown` with the error message as reason). Emits, in order: `session.attached` (capabilities below, `title = ref.title`, `meta {ref, footer}`), `session.window {mode:"tail"}`, `session.footer`, `session.status`, `session.dialog` for each open reconstructed dialog. Then `watchSession` starts from `endOffset` with the tail's normalizer.
- Live: `append` events emit `session.append` (`added`), `session.update` (`updated`), `session.dialog` / `session.dialogResolved` for `dialogsOpened` / `dialogsResolved` (outcome `"answered"`; plan review `superseded` or `lost` -> `"superseded"`), `session.footer` when `factsChanged`, `session.status` when `busy` changed. `reset` -> re-reads the tail, emits a fresh `session.window {mode:"tail"}` (truncation or replacement resets the window) and restarts the watcher.
- `busy` (Phase 1 choice, a file-derived approximation): `facts.lastRole === "user" || "toolResult"`, or `lastRole === "assistant"` with `lastStopReason` of `toolUse`/`pending`; false otherwise. `queued` is always `[]`.
- `loadEarlier(before)`: `readEarlier`, emits `session.window {mode:"earlier"}`; on a session attached with no file it emits an empty `session.window {mode:"earlier", items: [], hasEarlier: false, cursor: null}`.
- `submit`: images are written and referenced in the text (11.3); `whenBusy === "steer"` -> `unsupported_capability` (reason of `capabilities.steer`); **Amended by 16.2 (air-d43):** `text.trim() === ""` with no images, or `text.length > PI_SEND_TEXT_MAX`, -> `invalid_submission`; with images and no text the submission is valid: the images are written and the text sent is the `[Attached image: <path>]` lines alone, with no leading blank line (11.3); else `cpd.json(sessionSendArgs(id, text))`, `sessionSendArgs` passing `--origin webui` (11.9: the `session-message` Event carries `data.origin: "webui"`) (text-file attachments were already inlined by the browser, 6.9) and `session.submitAck {queued:false}`; a `cpd_failed` rejects and the hub reports it as `session.error {clientSubmissionId}`. After a submit that succeeds on a session attached with no file (pro-d397, pro-d400), the backend polls `cpd session show <id>` (`deps.cpd.json`) every 250 ms through the injected `deps.scheduler.after`, for at most 20 tries. When the reply's `session.file` is a non-empty string, it reads that tail as `attach` does, emits `session.window {mode:"tail"}`, `session.footer` and `session.status`, and starts the watch from `endOffset`. When the file never appears within the 20 tries, it emits `session.error` with `session_file_missing` `{sessionId}`. `dispose()` cancels the pending poll timer, so no timer outlives the handle.
- `configure {kind:"tier"}`: `cpd.json(sessionRetierArgs(id, tier))` (applies at the next idle, scout-pi §6), then re-reads `cpd session show` and emits `session.footer`. `{kind:"model"|"thinking"}` -> `unsupported_capability`.
- `abort`, `abortSubmission`, `compact` and `listModels` -> `unsupported_capability` with the reason of the matching capability (`listModels` -> the `configure.model` reason). `respond` answers the open dialog (11.4).
- Footer: `model` = `facts.model` (a later `model_change` wins over `session show`) else `session show` `model`; `thinking` likewise; `tier` = `session show` `tier` else `ref.tier`; `cost` = unknown (`"pi session cost needs a full-file scan, which Phase 1 never does"`); `contextPercent` = unknown (`"the model's context window is not available for pi sessions"`); `branch` = `git.branch(ref.cwd)` (the git branch of the session cwd; `ref.cwd` missing -> unknown `"the session row has no cwd"`; `null` -> unknown `"not a git repository"`).
- `dispose()` disposes the watcher; nothing is emitted afterwards.

### 6.6 hub-srv `web/src/server/hub/` and `web/src/server/plugin.ts`

#### `registry.ts`

```ts
export interface Registry { resolve(sessionId: string): ResolvedSession } // synchronous, from the current cockpit snapshot
export function createRegistry(deps: { snapshot: () => CockpitSnapshot; cpdHome: string }): Registry;
export const SOCKET_PATH_MAX = 107; // config.ts:31 (scout-durable §1)
export function socketPathOf(cpdHome: string, sessionId: string): string; // `${cpdHome}/run/host/${sessionId}.sock`
```

`resolve` walks `snapshot().tree` (`ContextNode.sessions` and every `PlanNode.sessions`) for `ref.id === sessionId`:

- not found, or `tree === null` -> `WebError("session_not_found", ..., {sessionId})` (no refresh is attempted);
- `runner "pi"` with no `ref.file` -> `{ref, tierOptions}` (no `file`, no error; pro-d397); otherwise `{ref, file: ref.file, tierOptions}`;
- `runner "durable"` -> `{ref, socketPath: socketPathOf(cpdHome, id), tierOptions}`; a path longer than `SOCKET_PATH_MAX` bytes -> `host_unreachable` with `details {reason:"socket_path_too_long"}` (the host would refuse it, `config.ts:31-32`);
- `tierOptions` per 6.1.

This resolves a session id to its runner, file and socket path.

#### `connection.ts`: one per browser

```ts
export interface HubDeps {
  cockpit: CockpitService; registry: Registry;
  backends: Readonly<Record<Runner, SessionBackend>>;
  send: (message: ServerMessage) => void;
}
export interface HubConnection { handle(raw: unknown): Promise<void>; close(): Promise<void> }
export function createHubConnection(deps: HubDeps): HubConnection;
```

Behaviours:

- `handle(raw)` runs `parseClientMessage(raw)` (the only parse of client data). A `WebError` becomes `send({type:"session.error", sessionId: null, error})`: invalid client messages are rejected with a typed error (`invalid_message`, or `unknown_action`), and nothing else happens. After `close()` every `handle` is a no-op.
- `cockpit.subscribe`: sends `cockpit.snapshot` from `cockpit.snapshot()` (when any pane is still `null` -> `send({type:"cockpit.error", error: internal "cockpit not ready"})` instead), then subscribes once to `cockpit.subscribe(...)`: later `pane` updates are sent as `cockpit.patch` and `error` updates as `cockpit.error` (subscribe then invalidation updates). A second subscribe re-sends the snapshot and does not add a second subscription.
- `cockpit.selectPlan` -> `await cockpit.selectPlan(planId)` (the plan pane arrives through the subscription as a patch); a failure -> `cockpit.error`.
- `cockpit.action` -> `await cockpit.runAction(action)`; success -> `action.result {ok:true, output}`, a `WebError` -> `action.result {ok:false, error}`.
- `session.attach {sessionId}`: detaches the previous session first (dispose its handle), then `registry.resolve` (a `WebError` -> `session.error {sessionId, error}`; e.g. `session_not_found`), then `backends[resolved.ref.runner].attach(resolved, emit)`, selecting the backend by session runner. `emit` forwards a message only while this attach is still current and the connection open (a generation counter): messages of a detached session never leak. An `attach` that rejects -> `session.error`; an attach that resolves after a newer attach/detach/close disposes its handle and emits nothing.
- `session.detach`: disposes the handle (once), forgets the session. Idempotent.
- `session.loadEarlier|submit|abort|abortSubmission|compact|configure|listModels` and `dialog.respond`: when `sessionId` is not the attached one -> `session.error {sessionId, error: not_attached}`; otherwise the matching `SessionHandle` method (session messages route to the attached backend). `submit` passes `{clientSubmissionId, text, images, whenBusy}`. A rejection becomes `session.error {sessionId, clientSubmissionId?, error: toWireError(e)}` (`clientSubmissionId` for `submit` only): backend errors become session error messages.
- **`/q` and `/quit` intercept (pro-d406; A3, A4).** In `session.submit`, when `text.trim()` is exactly `/q` or `/quit` (case-sensitive; `text` is the wire text, with text attachments already inlined, 6.9) and the session is a Manager, a Lead or a side fork, the hub does not submit it: no backend `submit`, no `session.submitAck`, nothing reaches the model, and images on that submit are not sent. The check runs at this one submit point, after the `not_attached` check, so both runners are covered (the tmux pi backend and the durable backend sit behind it). It never runs in the pi adapter (`pi/lib/turns.ts`) or the durable host (`pi/durable-host/run.ts`). Workers are not intercepted. By role:
  - **Manager:** refused without a cpd call: `session.error {sessionId, clientSubmissionId, error: {code: "manager_not_archivable", message: "A Manager is archived only with its Context.", details: {sessionId}}}`.
  - **Plan Lead** (the `leadSessionId` of its Plan, 5.1): runs `plan.archive` for its Plan. **Any other Lead or side fork:** runs `session.archive`. Both go through `runAction` (`actions.ts`, 5.4) with its park-and-retry for `in_chat_slot`, so `blockers()` is enforced exactly as for the cockpit.
  - **Success:** no reply for the submission and no `action.result`. The archive reaches the client as a tree change, which the web handles as DESIGN 3h says.
  - **Failure** (any `WebError` from `runAction`, `archive_blocked` included): `session.error {sessionId, clientSubmissionId, error: toWireError(e)}`, the same error the cockpit's `action.result` carries: `code "cpd_failed"`, `details.cpdCode` = cpd's code (`archive_blocked`, with `details.details.blockers` as a list of `{kind,id,reason,plan?}`), `details.exitCode` 3. **Amended by 16.1 (air-d44):** the pending row is dropped instead of being marked `failed` (6.2), and its error is shown once in the cockpit error toast (11.7); the User stays where they are.
- `close()` disposes the attached handle and the cockpit subscription; idempotent. Disconnect stops backend streams.

#### `plugin.ts`: the Vite plugin and the real boundaries (thin, ONE place)

```ts
import type { Plugin } from "vite";
export interface PluginBoundaries { cpdExec: CpdExec; follow: FollowBoundary; socketFactory: SocketFactory; fs: FileBoundary; watch: WatchBoundary; git: GitBoundary; scheduler: Scheduler; cpdHome: string }
export interface Services { cockpit: CockpitService; registry: Registry; backends: Record<Runner, SessionBackend>; newConnection(send: (m: ServerMessage) => void): HubConnection }
export function createServices(b: PluginBoundaries): Services; // pure wiring of 6.3-6.6; builds readmodel, cockpit service, registry, both backends (connect = connectHost(path, {factory, scheduler})); nothing is started
export function realBoundaries(env: NodeJS.ProcessEnv): PluginBoundaries; // child_process (cpd: `$CPD_BIN` else `cpd`; git), net sockets (unix), node:fs (stat/read ranges), fs.watch, timers; `cpdHome` = `$CPD_HOME` else `~/.cpd`. Untested glue, kept thin.
export function cpdWebPlugin(options?: { boundaries?: PluginBoundaries }): Plugin; // name "cpd-web"; `apply: "serve"` (never in `vite build`)
```

Behaviours (tested with a fake `ViteDevServer` and fake boundaries):

- `configureServer(server)` builds the services (`options.boundaries ?? realBoundaries(process.env)`) and starts them: `cockpit.start()` is called once; its promise is awaited before the first client message is handled (a rejection is logged through `server.config.logger.error` and every `cockpit.subscribe` is then answered with `cockpit.error`; there is no retry: restart the dev server).
- It registers `server.ws.on("cpd:c2s", (data, client) => ...)` (`vite index.d.ts:1833-1839`). The first message of a `client` creates its `HubConnection` with `send = (m) => client.send("cpd:s2c", m)` and registers `client.socket.on("close", ...)` to close it (`client.socket` `:1860-1866`); later messages of the same client reuse it. The plugin sends `s2c` to the client that sent the message only. **Amended by 15.1 (Plan `webui-ws`): this `cpd:c2s`/`cpd:s2c` route is gone** — the dev server hosts the one WebSocket endpoint of §15 on its httpServer instead, same-origin at `/cpd-ws`.
- `cpd:c2s` payloads are passed to `hub.handle(data)` unparsed; a rejected `handle` is logged, never thrown into Vite. **Amended by 15.1:** the same `hub.handle(data)` call serves the WebSocket endpoint's incoming text frames, and the hub answers a `ping` client message with `pong` at once (15.2), with no cockpit, backend or parse error involved.
- Closing: on `server.httpServer?.once("close")` (and the plugin's `closeBundle` hook, whose dev-server timing is unverified, O17) the plugin closes every connection, calls `cockpit.stop()`; idempotent. Plugin starts services on server start and closes them on server close.

`web/vite.config.ts`: `export default defineConfig({ plugins: [react(), cpdWebPlugin()], server: { port: 5173, strictPort: true } })`. No other plugin (no Tailwind), no other server middleware; the legacy live adapter is deleted with the legacy code.

### 6.7 ui-cockpit `web/src/ui/cockpit/`

Presentational: props in, callbacks out; no store, connection or network import (pro-d16: minimal semantic HTML, no polish). Each component renders the DOM below; tests query it with `querySelector`/roles.

```ts
export function ContextTree(props: { tree: ContextTreeModel | null; selectedSessionId: string | null; selectedPlanId: string | null; onSelectSession(sessionId: string): void; onSelectPlan(planId: string): void }): JSX.Element;
export function PlanView(props: { plan: PlanViewModel | null; postableWorkflows?: ReadonlySet<string>; onAction(action: CockpitAction): void }): JSX.Element; // postableWorkflows: pro-d77, DESIGN-panes 1f
export function ManagerView(props: { context: ContextNode | null; onSelectPlan(planId: string): void; onSelectSession(sessionId: string): void }): JSX.Element; // pro-d70, pro-d75; DESIGN-panes 1j
export function Inbox(props: { inbox: InboxModel | null; onAction(action: CockpitAction): void; onSelectSession(sessionId: string): void }): JSX.Element;
export function Shell(props: { tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode; header?: ReactNode; status?: ReactNode }): JSX.Element; // header, status: pro-d18; a slot is present when its prop is not `undefined`
export function Header(): JSX.Element; // pro-d18; no props (Phase 1 is the plain text `C-P-D`)
export function StatusBar(props: { runner: Runner | null; state: string | null; version: Stat<string> | null }): JSX.Element; // pro-d18; Runner, Stat from `protocol/domain.ts`
```

**ContextTree DOM.** `<nav aria-label="Context tree">`; `tree === null` -> text `Loading contexts…`. A `<ul role="tree">` of Contexts: `<li role="treeitem" data-context="<id>">` with the Context title; its Plans as nested `<li role="treeitem" data-plan="<id>">` containing `<button data-select="plan">` (label `plan.title`, plus counts text `merged 13 · running 1`: only non-zero counts, order draft, queued, running, merged, failed, cancelled); its sessions (Context sessions, then each Plan's sessions) as `<button data-session="<sessionId>">` with text `ref.key` and, when set, ` — <title>`, preceded by the state dot `<span role="img" data-dot="<state>" data-attention="<urgency|none>" aria-label="<state>[, needs you (<urgency>)]">`. The selected session button and the selected plan button carry `aria-current="true"`. Archived Contexts and archived Plans are placed inside a closed `<details data-archived><summary>Archived (n)</summary>…</details>`. Clicking a session button calls `onSelectSession(id)`; clicking a plan button calls `onSelectPlan(id)`. The session row menu's `Archive` item calls `clearDraft(<session id>)` (6.9) before dispatching `session.archive`; Plan and Context archive do not touch drafts.

**PlanView DOM.** **Amended by 14.2 (air-d28):** `plan === null` -> the text `Select a lead to see plans`, and nothing else (the empty state renders no flowchart: `CpdFlowCanvas` renders only in the Documentation dialog). Otherwise `<section aria-label="Plan <id>">`: `<h2>` title, phase, review status (`Review r1: approved`) when `review !== null`, `Escalations: n` when `openEscalations > 0`. Each Workflow `<section data-workflow="<key>" aria-label="Workflow <key>">` with key, status, repo and base, `#<number> <state>` as a link (`<a href=url target="_blank" rel="noopener noreferrer">`, ending with an `ExternalLinkIcon`, pro-d77) when `pr !== null`, and a `<button data-action="post-pr">Post PR</button>` only when `pr.posted` is not true, the PR is not merged and the Workflow is postable (`postableWorkflows`, when passed), which calls `onAction({kind:"pr.post", planId, workflow: key})`; inside, `<ul>` of its Jobs (by `jobKeys` order, looked up in `jobs`): `<li data-job="<key>" data-status="<status>">` with key, title, status, tier, and `needs: a, b` only when `needs` is non-empty (job needs); attempts as `#<number> <status>` text. Jobs with `workflow === null` go in `<section aria-label="Explore jobs">`. The `dag-body` of each Workflow is a `ScrollArea` with a horizontal `ScrollBar` (pro-d69).

**ManagerView DOM.** **Amended by 14.2 (air-d28):** `context === null` -> the text `Select a lead to see plans`, not `<CpdFlowCanvas />`. Otherwise `<section aria-label="Manager overview <id>">` with a sticky `<header data-part="manager-header">`: `<h2 data-part="manager-key">` (the Context id) and `<span data-part="manager-counters">` (`n active · n done · n records`), and no flowchart control. Its `<div data-part="manager-body">` holds one `<ol data-part="plans">` of every unarchived Plan, done Plans included, one `<li data-manager-plan="<id>" data-phase="<phase>">` each, in `livePlans(context)` order (`state/archiveNav.ts`; pro-d89). There is no Done collapsible. A Plan row is a `role="button"` holding, in order, its Lead's Dot, `plan-id`, `plan-title` (the flexible, truncating item), the PR chips (`wf-pr-chip` links; a `wf-pr-summary` `<n> PRs` HoverCard for more than two Workflows), `plan-phase`, `plan-actor` and `plan-counters`. A click, Enter or Space calls `onSelectPlan(id)` and then `onSelectSession(lead)`. DESIGN-panes 1j has the rules.

**Inbox DOM.** `<aside aria-label="Inbox">`: `<h2>Inbox (<openCount>)</h2>` (the open item count); `inbox === null` -> `Loading inbox…`; `engineRunning === false` -> `<p role="status">Engine is not running</p>`; zero rows -> `<p data-empty>Nothing needs you</p>`. The rows sit in a `ScrollArea` (pro-d69). Each row `<li data-kind="<kind>" data-urgency="<urgency>">`: `<span data-part="kind">kind</span>`, the label's first line in `<strong>` and further lines in `<span>`; a `<button data-action="open-session">` calling `onSelectSession(sessionId)` when `sessionId !== null`; an `<a href=url>` when `url !== null`; one `<button>` per `row.actions` entry (text = its `label`) calling `onAction(entry.action)`: row actions call `onAction` with the typed action.

**Shell DOM.** `<div data-shell>`: when `header` is given, its first child is `<header aria-label="Cockpit header">{header}</header>`; then left `<aside aria-label="Context tree column">{tree}</aside>`, centre `<div data-column="center"><section aria-label="Plan pane">{plan}</section><main aria-label="Chat">{chat}</main></div>`, right `<aside aria-label="Inbox column">{inbox}</aside>`; when `status` is given, its last child is `<footer aria-label="Status bar">{status}</footer>`. With neither slot given the DOM is exactly the 3-pane DOM above. The 3-pane layout of pro-d2 around the chat slot is unchanged by pro-d18; this arrangement is a Phase 1 choice. The content of the Plan pane section is wrapped in a `ScrollArea` (pro-d69): `section[aria-label="Plan pane"] > [data-slot="scroll-area"] > [data-slot="scroll-area-viewport"]`. The app stylesheet (6.11) places the header and footer across all columns; that is CSS, not DOM.

**Header DOM (pro-d18).** `<span data-emblem>C-P-D</span>`. `Shell` supplies the `<header>` wrapper.

**StatusBar DOM (pro-d18).** Three `<span>` children, in this order, with no wrapper (`Shell` supplies the `<footer>`): `<span data-part="runner">Runner: <runner|none></span>`, `<span data-part="state">State: <state|none></span>`, `<span data-part="version">cpd: <version|unknown></span>`. `version` shows its value only when it is `{state:"known"}`; `null` or `{state:"unknown"}` shows `unknown`. Phase 1 has no styling and no left/right split of pills (pro-d16, pro-d18). **Superseded by docs/ui-preview.md, Production and preview modes (item 6):** the version is `<Badge variant="ghost" data-part="version">` with the bare version text, with no `cpd: ` prefix; preview mode adds `<Badge variant="outline" data-part="mode">preview</Badge>` after it and a `data-part="drift"` pill (`drift N`).

**Behaviours to test (pro-d18).** Shell: with `header` and `status` given, `[data-shell]`'s first element child is the `header` and its last is `footer[aria-label="Status bar"]`, with the three panes between in order; with neither given the child list is the 3-pane list. Header: `[data-emblem]` text is `C-P-D`. StatusBar: runner `pi` shows `Runner: pi`, `null` shows `Runner: none` (likewise State); a known version `0.21.102` shows `cpd: 0.21.102`; `null` or unknown shows `cpd: unknown`. **Superseded (docs/ui-preview.md item 6):** the known version shows its bare text (`0.21.102`), without the `cpd: ` prefix.

### 6.8 ui-chat `web/src/ui/chat/`

> **Superseded by 10.4, 10.5 and 10.6:** the `Transcript` signature (new props `tailEpoch`, `sizes`; `PendingSubmission` imported from `state/store.ts`), the Transcript DOM (virtual space and window, `data-row-key`, the `Jump to bottom` button, `data-at-bottom`), tool grouping (a `tools` row of `groupRows` replaces the `tool-group` `<details>` wrapper), and the text rendering of assistant blocks, `FoldRow` and `CompactionRow` (through `Markdown`, 10.6). The `FoldRow` text "`cpd-context`, `cpd-plan-decision`, `cpd-skills` are closed by default" also holds for fold kind `cpd` (closed), except the compaction-failure fold of 6.4 live tail, L6 (open).

```ts
export function Transcript(props: { items: readonly TranscriptItem[]; hasEarlier: boolean; queued: readonly QueuedSubmission[]; pending: readonly PendingSubmission[]; onLoadEarlier(): void }): JSX.Element;
export function ToolRow(props: { tool: ToolItem }): JSX.Element;
export function OrphanRow(props: { item: OrphanResultItem }): JSX.Element;
export function FoldRow(props: { item: FoldItem }): JSX.Element;
export function CompactionRow(props: { item: CompactionItem }): JSX.Element;
export function ChatHeader(props: { sessionKey: string; title: string | null }): JSX.Element;
export function Footer(props: { footer: FooterStats | null; contextWindow?: number }): JSX.Element; // pro-d57: the context card only (11.8)
```

**Transcript DOM.** `<div role="log" aria-label="Transcript">`. When `hasEarlier`: first child `<button data-action="load-earlier">Load earlier</button>` calling `onLoadEarlier` (absent when `hasEarlier` is false). Items in order:

- user: `<div data-kind="user">`, a shadcn `Message` with a muted `Bubble` holding the text (whitespace preserved), image thumbnails when `dataBase64` is present, and `You · HH:MM` (DESIGN-chat 2.3, 2.3.1; pro-d56, pro-d57, pro-d61).
- assistant: `<div data-kind="assistant">` with muted Bubbles for text blocks (Markdown). Thinking blocks are rows of the Chain of Thought (DESIGN-chat 2.4.2, pro-d61). There is no session stamp (pro-d59).
- tool steps: a run of consecutive thoughts and tools is one collapsed Chain of Thought row (DESIGN-chat 2.4.2); inside it two or more consecutive tools form a Tool Group (DESIGN-chat 3.4).
- after the transcript: `<ul aria-label="Queued messages">` with one `<li data-queued="<clientSubmissionId>">` per queued submission (outline bubble, dashed text, `queued`) and one `<li data-pending="<state>">` per pending submission (DESIGN-chat 2.6, 2.7).

**ToolRow DOM** (scout-pi §4, compact-tools). `<details data-kind="tool" data-tool="<name>" data-status="<status>">`, `open` attribute set only for `name === "edit"` (edit's body shows as a bounded preview by default, scout-pi §4). `<summary>`: `<span data-part="name">` (name with a leading `mcp__` removed, so `mcp__srv__tool` shows `srv__tool`), `<span data-part="detail">`, `<span data-part="status">`:

- **detail** by tool: the table of DESIGN-chat 3.2 (paths and commands cut at 100 characters, other details at 160).
- **status** text: `running` -> `Running`; **Amended by 16.1 (air-d44, air-d77):** `failed` -> `Failed`, with no inline reason in the status text — no toast is raised for a failed agent tool call (16.1): the failure's record is the row itself, which keeps its failed tone and the tool group's failed count; `done` -> `Done`, plus ` in <duration>` when `durationMs` is a number (`<n>ms` below 1000, else seconds with one decimal and `s`), plus ` (+<added> -<removed>)` for `edit` (counts of `+`/`-` diff lines, not `+++`/`---`) or ` (<N> lines)` for the others (`1 line` singular; `N` = number of lines of `result.text`, a trailing newline not counted, empty text = `0 lines`).
- **body** (inside the row's closed `TraceCollapsibleContent`): DESIGN-chat 3.6 (a `CodeBlock`, the first 10 lines, `Show N more lines`).

`OrphanRow`: `<article data-kind="orphan-result">` with `Result of <name> (call not loaded)` and the same output body rules.

**FoldRow DOM.** `<details data-kind="fold" data-fold="<foldKind>">` with the attribute `open` iff `item.defaultOpen`; `<summary>{title}</summary>` then the body as ReactMarkdown. So `cpd-context`, `cpd-plan-decision`, `cpd-skills` are closed by default and `cpd-plan-summary` is open (`pi/lib/render.ts:212`). **CompactionRow**: `<details data-kind="compaction"><summary>Compaction</summary>` plus `(<tokensBefore> tokens)` in the summary when known, body = `summary` text; closed (compaction renders as a fold).

**ChatHeader.** `<header><h1 data-part="key">{sessionKey}</h1><span data-part="title">{title}</span></header>` (`title` omitted when null). Sets `document.title = title === null ? sessionKey : sessionKey + " · " + title` in an effect (updates when the props change) and restores the title it found at mount on unmount (pro-d10 item 5).

**Footer DOM.** `<footer aria-label="Session footer">` with six `<span data-stat="model|tier|thinking|context|cost|branch">` each containing its label and the value: model/tier/thinking/branch as the string, context as `<n>%`, cost as `$<value to 3 decimals>`. An `unknown` stat renders `<span data-state="unknown" title="<reason>">unknown</span>` (explicit marker, never blank or fake). `footer === null` renders all six as unknown with reason `not attached`.

### 6.9 ui-composer `web/src/ui/composer/`

#### `attachments.ts`

```ts
export interface FileLike { readonly name: string; readonly type: string; readonly size: number } // structurally a DOM File
export interface FileReaderBoundary { readBase64(file: FileLike): Promise<string>; readText(file: FileLike): Promise<string> } // padded base64 without the data: prefix
export type RejectionReason = "too_many_images" | "images_too_large" | "unsupported_image_type" | "images_unsupported" | "binary_not_supported" | "text_too_large";
export interface AttachmentRejection { readonly name: string; readonly reason: RejectionReason; readonly message: string }
export interface AddResult { readonly accepted: Attachment[]; readonly rejected: AttachmentRejection[] }

export function addFiles(files: readonly FileLike[], existing: readonly Attachment[], capabilities: Capabilities | null, reader: FileReaderBoundary): Promise<AddResult>;
export function filesFromClipboard(data: { files: ArrayLike<FileLike> } | null): FileLike[]; // paste event `clipboardData`
export function filesFromDrop(data: { files: ArrayLike<FileLike> } | null): FileLike[]; // drop event `dataTransfer`
export function buildPrompt(text: string, attachments: readonly Attachment[]): { text: string; images: WireImage[] };
```

`addFiles` processes `files` in order and never throws (a boundary read failure rejects with the original error):

- classify: `type` in `IMAGE_MIME_TYPES` -> image; any other `image/*` type -> reject `unsupported_image_type`; text-like -> text; else reject `binary_not_supported` (binary non-image files are rejected with a reason). Text-like = `type` starts with `text/` or is one of `application/json`, `application/xml`, `application/x-yaml`, `application/toml`, `application/javascript`, `application/typescript`; or `type` is `""`/`application/octet-stream` and the lowercase extension is one of `md txt json ts tsx js jsx mjs cjs rs toml yaml yml csv log sh css html xml py go java c h cpp sql` (Phase 1 choice).
- image with `capabilities !== null && !capabilities.images.supported` -> reject `images_unsupported`, `message` = the capability reason.
- image count: `existing` images + accepted images in this call may not exceed `IMAGE_MAX_COUNT` (4); further images are rejected `too_many_images` (more than four images are rejected; the first four are kept).
- image size: after `reader.readBase64`, if the sum of `dataBase64.length` over existing + accepted + this file exceeds `IMAGE_MAX_TOTAL_BASE64` (786432) the file is rejected `images_too_large` (total base64 beyond the limit is rejected; earlier files stay). Accepted: `{kind:"image", name, mimeType, dataBase64}`.
- text: `size > TEXT_INLINE_MAX_BYTES` -> reject `text_too_large`; else `{kind:"text", name, text: await reader.readText(file)}`.
- `filesFromClipboard` / `filesFromDrop`: `Array.from(data.files)`; `null` -> `[]` (paste and drop events yield the attached files).

`buildPrompt(text, attachments)`: `text` = the user text, then for each text attachment in order a blank line and `<file name="<name>">\n<content>\n</file>` (the wrapper pi itself uses for file input, scout-pi §1 line 7; `"` in the name becomes `&quot;`); text files are inlined into the prompt. `images` = the image attachments as `WireImage` in order. With no text attachments `text` is returned unchanged.

#### `drafts.ts`

```ts
export function loadDraft(sessionId: string): string; // stored text; "" when none or storage unavailable
export function saveDraft(sessionId: string, text: string): void; // stores; an empty text removes the key
export function clearDraft(sessionId: string): void; // removes the key
```

The draft is the composer text of one session, kept in the browser's localStorage under the key `cpd-ui:draft:<sessionId>` (`sessionId` = `SessionView.id` = `ref.id`). Only the text is stored, never attachments. An empty text removes the key, so there is at most one draft per session. Storage errors (unavailable or throwing localStorage) are swallowed: `loadDraft` returns `""`, and the Composer keeps in-memory behaviour, as in `cockpit/persisted.ts`.

#### `Composer.tsx`

```ts
export interface ComposerHandle { addFiles(files: readonly FileLike[]): Promise<void> } // the chat slot forwards dropped files here
export function Composer(props: {
  capabilities: Capabilities | null; busy: boolean; queued: readonly QueuedSubmission[];
  reader: FileReaderBoundary; sessionId: string;
  onSubmit(input: { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy }): void;
  onAbort(): void; onCancelQueued(clientSubmissionId: string): void;
  ref?: Ref<ComposerHandle>; // React 19 ref prop
}): JSX.Element;
```

DOM and behaviour:

- Structure and keys: DESIGN-compose 1.2 and 1.4. `textarea[aria-label="Message"]`: Enter submits `{text, attachments, whenBusy: "followUp"}` and clears; Shift+Enter is a newline; IME composition is ignored.
- **Draft.** The textarea starts with `loadDraft(sessionId)`. Every text change calls `saveDraft(sessionId, text)` in the change handler (not in an effect). A send (Enter or the Send button) calls `clearDraft(sessionId)`. When `sessionId` changes, the textarea shows `loadDraft(<new sessionId>)` (the previous session's draft stays stored, unchanged), and `attachments` and `rejections` reset to empty.
- **Send**: `button[data-action="send"][aria-label="Send"]`, disabled when there is nothing to send or `capabilities` is `null`; a click submits `whenBusy: "followUp"`.
- **Busy**: the Stop button, `button[data-action="abort"][aria-label="Stop"]`, is shown while busy when `capabilities.abort.supported` (pro-d402, which supersedes the Stop removal of pro-d57; DESIGN-compose 1.2); its click calls `onAbort` (`connection.abort`). It is absent when not busy and for pi sessions (abort unsupported, 8). The prompt border pulses (`prompt-border-pulse`, DESIGN.md 2h) while busy or a send is pending. Send submits a follow-up. There is no Steer or Follow-up button (pro-d57).
- **Unsupported capabilities**: the attach button and the file picker follow `capabilities`. There is no steer control to disable (pro-d57).
- **Queued submissions**: header chips `ul[aria-label="Queued submissions"]` with `li[data-queued-chip]`, and `button[data-action="cancel-queued"]` when `abortSubmission` is supported.
- **File picker**: the footer's `Attach files` button opens a hidden `input[type="file" multiple]`; its `change` calls `addFiles`.
- **Paste**: on the textarea, a paste whose `clipboardData.files` is non-empty calls `preventDefault` and adds those files; text pastes are untouched.
- **Drag and drop**: the chat dropzone (`ChatPane`, `data-chat-dropzone`) takes the drop and calls the Composer's `addFiles` once. The Composer shows `Drop to attach` while files are dragged over it (`data-drop="over"`, DESIGN-compose 1.2).
- Image attachments are only accepted per `addFiles` (capability, count, size).

#### `TierSelector.tsx`

```ts
export type ConfigureHandler = (change: ConfigureChange) => void;
export function TierSelector(props: { capabilities: Capabilities | null; footer: FooterStats | null; models: readonly ModelChoice[] | null; busy: boolean; onConfigure: ConfigureHandler; onRequestModels(): void }): JSX.Element;
```

`<div data-part="tier-selector">` with three shadcn `DropdownMenu`s with radio items on `InputGroupButton`s: `aria-label="Tier"`, `"Model"`, `"Thinking level"` (DESIGN-compose 2, pro-d55). The option rules below apply to the menus:

- `aria-label="Tier"`: options = `capabilities.configure.tier.options` (plus the current known tier when missing from them); value = the footer tier when known. `change` -> `onConfigure({kind:"tier", tier})`. Not disabled by `busy` (cpd applies a Tier change at the next idle).
- `aria-label="Model"`: options = `models` (value `model`, text `name`); before they are loaded one option showing the current model (or `unknown`); the first `focus` calls `onRequestModels()` once. `change` -> `onConfigure({kind:"model", model})`. Disabled while `busy`, with reason `A run is pending: the model changes when the session is idle` (the host refuses configure while a run is pending, scout-durable §7.4).
- `aria-label="Thinking level"`: options = `thinkingLevels` of the model matching the footer model in `models` (plus the current level); `change` -> `onConfigure({kind:"thinking", level})`; same busy rule as Model.
- Any menu trigger whose capability is `{supported:false, reason}` (`tier`, `model`, `thinking`) is `disabled` with `title="<reason>"`. `capabilities === null` disables all three. The lists of tiers, models and levels are as above.

### 6.10 ui-dialogs `web/src/ui/dialogs/`

```ts
export function AskCard(props: { dialog: Extract<Dialog, { method: "ask_user_question" }>; readOnly: boolean; onRespond(response: DialogResponse): void }): JSX.Element;
export function PlanReviewCard(props: { dialog: Extract<Dialog, { method: "cpd_plan_review" }>; readOnly: boolean; onRespond(response: DialogResponse): void }): JSX.Element;
```

`readOnly` is `!dialog.interactive` (pi sessions, A4); the parent passes it explicitly.

**AskCard** (answer shape per scout-pi §5b-5c).

- Interactive DOM: DESIGN-compose 4.2: `Questionnaire[aria-label="Questions"][data-dialog="ask_user_question"]`, one `QuestionnaireItem[data-question]` per question, options `[data-option]`, a preview pane, Cancel `[data-action="cancel"]`, Next, Previous, Skip and Submit (pro-d56). The answer payload is unchanged (below).
- **Preview side by side**: `div[data-layout="side-by-side"]` when any option has a `preview` (DESIGN-compose 4.2).
- **Custom text**: the `Type something.` input of each question (DESIGN-compose 4.2).
- **Esc**: `Escape` inside the Questionnaire calls `onRespond({method:"ask_user_question", outcome:"cancelled"})`; the Cancel button does the same.
- **Submit**: the Questionnaire's submit builds `{answers, cancelled:false, unanswered?}` as below and calls `onRespond({method:"ask_user_question", outcome:"answered", result})`; a question with nothing chosen goes to `unanswered`.
- **Read-only**: `<section aria-label="Questions" data-dialog="ask_user_question" data-readonly>` with `<ol>` of questions (`header: question`, options as `<li>` with label and description, previews as text), no inputs, no buttons, plus `<p>` `Answer in the session's own terminal; this view is read-only.`

**PlanReviewCard** (outcome mapping onto the wire: 3.8, derived from `plan-review.ts`, never invented).

- Interactive DOM: DESIGN-compose 5.3: `Item` section `aria-label="Plan review" data-dialog="cpd_plan_review"` with `h3` = title, the summary as Markdown, `ItemActions` buttons `[data-action="approve"]`, `[data-action="request-changes"]`, `[data-action="reject"]` (and `diff` when passed), and the feedback `Textarea[aria-label="Feedback"]` with `[data-action="send-feedback"]`. Outcomes unchanged.
- **No plan summary**: the plan body section is always rendered, even when `summary` is empty. When `summary.trim() === ""` the card shows the notice `No plan summary was rendered; approval is disabled.` and disables `[data-action="approve"]` (Approve & lock) and the Cmd/Ctrl+Enter shortcut; Change and Cancel keep working.
- **Read-only**: title and summary only, no buttons, no textarea.

### 6.11 app `web/src/ui/App.tsx`, `ChatPane.tsx`, `web/src/main.tsx`

> **Superseded by 10.5 and 10.7:** `app.css` is no longer only "the Shell grid, side-by-side preview, diff colours, disabled styling": it also carries the structural scroll CSS of 10.5; `App` subscribes with separate selectors and wraps `ContextTree`, `PlanView` and `Inbox` in `memo` (10.7).

```ts
export interface AppProps { transport: Transport; scheduler: Scheduler; reader: FileReaderBoundary; newId?: () => string }
export function App(props: AppProps): JSX.Element; // creates ONE store and ONE connection (effect: start on mount, close on unmount)
export function ChatPane(props: { view: SessionView; connection: Connection; reader: FileReaderBoundary }): JSX.Element;
```

`main.tsx` (untested): builds the real `Transport` over `import.meta.hot` (6.1), the real `Scheduler` and a browser `FileReaderBoundary` (`FileReader`/`arrayBuffer` to padded base64 and `text()`), imports `app.css` and mounts `<App>` into `#root` (`web/index.html` keeps a `<div id="root">` and `<script type="module" src="/src/main.tsx">`). `web/src/ui/app.css`: one small plain CSS file: the Shell grid, side-by-side preview, `[data-diff="add"]`/`remove` colours, disabled styling; nothing else (pro-d16). The App takes the `Transport` as a prop, so the integration test uses `FakeTransport`.

Wiring:

- `ContextTree`: `onSelectSession` -> `connection.selectSession(id)`, `onSelectPlan` -> `connection.selectPlan(id)`; `PlanView.onAction`, `Inbox.onAction` -> `connection.action(action)`; `Inbox.onSelectSession` -> `connection.selectSession`.
- Plan pane: `visiblePlan` renders `PlanView`; otherwise `ManagerView` (pro-d70, pro-d75) with the Context that holds the selected session, `onSelectPlan` -> `connection.selectPlan`, `onSelectSession` -> `connection.selectSession`; with no such Context, the text `Select a lead to see plans` (14.2), not `CpdFlowCanvas`.
- Chat slot (`selectedSessionId !== null`): `ChatPane` composes `ChatHeader` (`ref.key`, `title`), the attach status line (`data-attach-status`: shown while `session.attached` is false and `session.error` is set; it renders the error `code` and `message` and a `Retry` button, `data-action="retry"`, that calls `connection.selectSession(selectedSessionId)`; hidden otherwise, 6.2 Retry), `Transcript` (`onLoadEarlier` -> `connection.loadEarlier()`), the active dialog card (`AskCard`/`PlanReviewCard` with `readOnly = !dialog.interactive`, `onRespond` -> `connection.respond(dialog.id, response)`), `TierSelector` (`onConfigure` -> `connection.configure`, `onRequestModels` -> `connection.listModels`), `Footer`, `Composer` (`sessionId={view.id}`; `onSubmit({text, attachments, whenBusy})` -> `const {text: prompt, images} = buildPrompt(text, attachments); connection.submit({text: prompt, images, whenBusy})`; `onAbort` -> `connection.abort`; `onCancelQueued` -> `connection.abortSubmission`). No session selected: `<p data-empty>Select a session</p>`.
- **Drop zone**: the chat slot wrapper `<div data-chat-dropzone>` handles `dragover`/`drop` with files (same `Files` rule as the Composer) and forwards `filesFromDrop(dataTransfer)` to the Composer's `ComposerHandle.addFiles` (the drop works onto the composer and onto the chat slot).
- **Header and status bar (pro-d18).** `Shell` gets `header={<Header />}` and `status={<StatusBar runner={r} state={s} version={CPD_VERSION} />}`. `r` and `s` are `ref.runner` and `ref.state` of the `SessionNode` whose `ref.id === selectedSessionId`, found by walking `cockpit.tree` (each Context's `sessions`, then each of its `plans[].sessions`); both are `null` when the tree is `null`, no session is selected, or the session is not in the tree. The tree is the one source: `session.ref` is set at attach only and is not refreshed by cockpit patches. `CPD_VERSION` is a module constant of type `Stat<string>`, `{state:"unknown", reason:"cockpit snapshot carries no cpd version (Phase 1, O19)"}`: the snapshot has no version field (1.4, unverified source O19). The ChatPane keeps `ChatHeader` at the top of the chat zone (no change).

Behaviours the App test covers (all through `FakeTransport`, `FakeScheduler`, a fake `FileReaderBoundary`):

1. On start the app subscribes: after `emitStatus("open")` the transport has sent `{type:"cockpit.subscribe"}`; after the server's `cockpit.snapshot` the three panes (tree, plan area, inbox) render.
2. Clicking a session in the tree sends `{type:"session.attach", sessionId}` (preceded by `session.detach` when another session was attached); after `session.attached` and `session.window {mode:"tail"}` the transcript tail renders in the chat slot.
3. Typing a message and pressing Enter sends `{type:"session.submit", sessionId, clientSubmissionId, text, images:[], whenBusy:"followUp"}`, except a text of exactly `/q` or `/quit`, which the hub intercepts instead (6.6).
4. A `session.dialog` message renders the dialog card; answering it sends `{type:"dialog.respond", sessionId, dialogId, response}`.
5. Dropping an image file on the chat slot adds the attachment (reader returns base64) and the next submit carries it as `images:[{mimeType, dataBase64}]`.
6. (pro-d18) Before any selection the header `[data-emblem]` reads `C-P-D` and the status bar reads `Runner: none` and `State: none`. After the snapshot holds a session with `runner "durable"` and `state "idle"` and that session is selected, the status bar reads `Runner: durable`, `State: idle` and `cpd: unknown`. **Superseded (docs/ui-preview.md item 6):** the version is shown bare when the snapshot carries it.

## 7. Error codes (typed, exhaustive)

> **Superseded by 10.8:** the `unsupported_host_version` row: the condition is "the pair (`welcome.protocol`, `welcome.pi_durable`) is neither (1, "1.3.0") nor (2, "1.5.0")".

`ErrorCode` of 3.9; every `WebError` and every `WireError` uses exactly these codes. Where raised and what `details` carries:

| Code | Raised by | When | `details` |
|---|---|---|---|
| `protocol_error` | `parseServerMessage`, `parseHostFrame`, `frames.ts`, `transcript.applyDelta` | a frame, line, event or server message does not match its schema; unsupported dialog method; out-of-range delta | `{field?, line?}` |
| `unsupported_host_version` | `client.ts connectHost`, `frames.assertSupportedHost` | `welcome.pi_durable !== "1.5.0"` or `welcome.protocol !== 2` (A3); an `error protocol_mismatch` answering the hello | `{piDurable, protocol}`, or `{host, client}` for the mismatch |
| `host_unreachable` | `client.ts`, `registry.ts`, `durable backend` | socket cannot be opened, no `welcome` in 5000 ms, no socket path, path > 107 bytes; a durable attach retries a connect rejection for 15000 ms before it fails (6.4 backend) | `{path?, reason?}` |
| `host_disconnected` | `client.ts`, `durable backend` | socket closed or `bye` while attached; pending requests rejected | `{bye?: reason}` |
| `host_error` | `client.ts` | the host answered `error` (e.g. `refused` for configure while busy, `bad_args`, `busy`, `unsupported`) | `{hostCode, details?}` |
| `session_not_found` | `registry.ts` | id absent from the cockpit snapshot | `{sessionId}` |
| `session_file_missing` | `pi backend` | pi session whose transcript file did not appear within the 20 polls after a submit (6.5) | `{sessionId}` |
| `session_file_unreadable` | `window.ts`, `watch.ts` | `stat`/`read` failed | `{path, cause}` |
| `malformed_line` | `jsonl.ts` | invalid JSON, non-object, bad `type`, wrong field types, `compaction` without `summary` | `{line?, offset}` |
| `cpd_failed` | `cli.ts` | non-zero exit or spawn failure | `{cpdCode, exitCode?, details?}` (`cpdCode` from the cpd JSON error, `no_json`, or `spawn_failed`) |
| `cpd_malformed_json` | `cli.ts`, `readmodel.ts`, `events.ts parseEventLine` | cpd output is not the expected JSON | `{args?, command?, field?}` |
| `invalid_message` | `parseClientMessage`, `parseCockpitAction`, backends | a wire message failed validation; unknown dialog/submission/cursor id | `{field?}` |
| `unknown_action` | `parseCockpitAction`, `actions.runAction` | action `kind` not in 5.4 | `{kind}` |
| `unsupported_capability` | `pi backend`, `hub` | a method the session's capabilities declare unsupported; `message` is the capability reason | `{capability}` |
| `invalid_submission` | `backend.submit` | empty text without images; pi text over 20 000 chars | `{max?}` |
| `tool_failed` | nothing raises it (16.1, amended by air-d77) | a tool result with status `failed` is applied to the selected session's transcript; no error and no toast is raised — the failure shows as a failed tool row (16.1) | — |
| `not_attached` | `hub/connection.ts`, `state/connection.ts` | a session message names a session that is not the attached/selected one | `{sessionId}` |
| `manager_not_archivable` | `hub/connection.ts` | a `session.submit` of `/q` or `/quit` in a Manager session (6.6); no cpd call is made | `{sessionId}` |
| `internal` | anywhere | a non-`WebError` throwable (via `toWireError`); `cockpit not ready`; send on a closed transport | `{}` |

## 8. Capabilities: durable vs pi

> **Amended by 12:** the pi "transcript source" row also has a live source, the pi live-stream socket (section 12), reconciled by item id with the JSONL tailer.

> **Superseded by 10.8:** the column header "durable (host socket, pi-durable 1.5.0 protocol 2)" holds for both dialects (v1 pi-durable 1.3.0 and v2 pi-durable 1.5.0); the "transcript source" row is v2 only (v1: `snapshot` and `events` frames).

`Capabilities` values per runner (fixed by the backends; `reason` strings are human text, tests match the `supported` flag and that `reason` is a non-empty string):

| Capability | durable (host socket, pi-durable 1.5.0 protocol 2) | pi (session JSONL) |
|---|---|---|
| `images` | supported; `maxCount 4`, `maxTotalBase64 786432`, png/jpeg/gif/webp (`protocol.ts:19-21`) | **supported** for pi from the approved preview: each image is written to the attachments directory and sent as `[Attached image: <path>]` (11.3) |
| `steer` | supported (`when_busy: "steer"`, `protocol.ts:42`) | **unsupported**: `cpd session send has no steer mode` (A4; O6) |
| `followUp` | supported (`when_busy: "followUp"`) | supported: delivered by `cpd session send`; whether the engine steers or follows up a busy session is unverified (O6) |
| `abort` | supported (`abort {background:false}`) | **unsupported**: `cpd has no "session abort" command (scout-cockpit §3)` (A4) |
| `abortSubmission` | supported (`abort_submission`) | **unsupported**: `pi sessions have no submission queue` |
| `compact` | supported (`compact`) | **unsupported**: `no cpd command compacts a pi session` |
| `configure.model` | supported (`configure {model}`); refused by the host while a run is pending (UI disables it while busy) | **unsupported**: `a pi session's model follows its Tier; change the Tier instead` |
| `configure.thinking` | supported (`configure {thinking_level}`), same busy rule | **unsupported**: same reason as model |
| `configure.tier` | supported via `cpd session retier <id> --tier <T>` (applies at idle); `options` = `tierOptions` | supported via the same command |
| `dialogs` | `{mode:"interactive"}` (`ui_request` / `ui_response`) | `{mode:"interactive"}` as well: `ask_user_question` and plan review are answered from the WebUI (11.4) |
| `queued` submissions | tracked from `submit` results and `inbox` (6.4) | always `[]` |
| footer cost | sum of `docs['pi.usage']` models' `cost.total` (cumulative, 6.4) | `unknown` (no full-file scan) |
| footer context % | `lastContextTokens / context_window` (approximation) | `unknown` |
| footer model/thinking | `docs['pi.agent']` and `host_state.agent` | `cpd session show` + `model_change` / `thinking_level_change` lines |
| footer branch | `git.branch(welcome.cwd)` | `git.branch(ref.cwd)` |
| transcript source | host `view` and `ops` frames, server-side cache and window (6.4) | JSONL tail by byte range, live by file watch |
| plan review Reject | `cpd plan decide --reject` via `CpdExec` (O9) | available from the WebUI (11.4): `cpd plan decide --reject --plan <planKey>` |

## 9. Open items

> **Amended by 10.9:** the unverified v2 items of Phase 2 are listed in 10.9 and are not renumbered here. O13 (the 1.5.0 shapes) additionally holds for the v2 dialect of 10.8, which is tested only against mocked frames.

Every unverified item of this file, listed once. A Job that needs one of them settles it (read the source, run the command) and amends this file through the Lead; no Job guesses.

| Id | Item | Where used | Source of the doubt |
|---|---|---|---|
| O1 | `custom_message` lines: fields beyond `type, customType, details` (`content`, `display`) are inferred from `pi.sendMessage` arguments; no sampled file line has one | 6.5 `jsonl.ts` | scout-pi §1 |
| O2 | `compaction` JSONL entry shape (`summary`, `firstKeptEntryId`, `tokensBefore`) is documentation only | 3.3, 6.5 | scout-pi §1 (`docs/research/webui-chat-base.md:199`) |
| O3 | `details.patch` / `details.diff` of an `edit` toolResult in a real file (no `edit` call in the sampled file) | 3.3 `ToolItem.diff`, 6.5 | scout-pi §2 |
| O4 | JSON of a durable session row in `cpd session list` (no durable Session exists on this machine; `file` is expected null, `place` none) | 5.1 | scout-cockpit §4 |
| O5 | `key` of a fork session is null by documentation; no fork exists live | 3.2 | scout-cockpit §4 |
| O6 | `cpd session send` on a busy pi session (steer or follow-up?), whether the web User is an allowed caller (help lists Lead and Manager), and whether `--` before the positional text is accepted | 6.3 `sessionSendArgs`, section 8 | scout-pi §6, scout-cockpit §3 |
| O7 | No cpd command lists the Tiers a session may take (`tier_not_allowed` carries `allowed` only on refusal); `tierOptions` is the observed distinct tiers | 3.6, 6.1, 6.6 | scout-cockpit §3 |
| O8 | pi-core's footer formulas (context %, cost, git branch) are not in the cpd repo; Phase 1 values are approximations or `unknown` | 3.5, 6.4, 6.5 | scout-pi §6 |
| O9 | Whether the web User may call the hidden `cpd plan decide --reject`, used for the Reject outcome of a plan review | 3.8, 5.4 | scout-cockpit §3 (internal command) |
| O10 | Whether the web User may call `cpd pr post` (caller listed as Lead) | 5.4 | `docs/commands.md:275` |
| O11 | Runtime timing of Vite's `vite:ws:connect` / `vite:ws:disconnect` events and whether the page reloads on reconnect (types declared in `vite/types/customEvent.d.ts:16-17`) | 6.1 | vite 8.3.5 typings only |
| O12 | `cpd session list` excludes archived sessions by default (inferred from the `--archived` flag) | 5.1 | `cpd session list --help` (scout-cockpit §1.1) |
| O13 | The 1.5.0 `view` and `ops` shapes were verified by a real Harness run for text and entries only (`live-check.md`), not through the host socket. The thinking and tool-argument append paths are unverified (O20) | 6.4 | `$CPD_HOME/run/pi-1-5-upgrade/explore/live-check.md`; `node_modules/@earendil-works/pi-durable/dist/harness/view.d.ts` |
| O14 | Whether durable `cpd-context` entries carry `data.summary` (the fold title then falls back to the first body line) | 6.4 | scout-durable §5 |
| O15 | `urgency` values beyond `red` and `magenta` and inbox kinds beyond those captured come from docs, not a live capture | 5.1, 5.3 | scout-cockpit §1.3, `docs/commands.md:3910` |
| O16 | Event kind names not written in `docs/commands.md` (the core `Kind` strings were not read) | 6.3 `panesFor` | scout-cockpit §2 |
| O17 | Whether Vite calls the plugin `closeBundle` hook on dev-server close (the plugin also listens on `httpServer` `close`) | 6.6 `plugin.ts` | vite 8.3.5 not read |
| O18 | Live output of `cpd events follow` (not run) and its `cursor_ahead` refusal on restart | 6.3 `events.ts` | scout-cockpit §2 |
| O19 | Source of a cpd version for the StatusBar. No cockpit read model carries one (`CockpitSnapshot`, 5). `cpd --version` is observed (scout-cockpit header, cpd 0.21.102), but no Phase 1 source reads it; whether a read command or snapshot field exists is unverified. Phase 1 shows `cpd: unknown`. **Superseded (docs/ui-preview.md item 6):** `status.ts` reads it and `StatusModel.version` carries it | 6.7 `StatusBar`, 6.11 | scout-cockpit header; the wider question is unverified |
| O20 | The `['a']` append paths of thinking blocks (`docs['pi.live'].generation.message.content[i].thinking`) are assumed from the pi-ai thinking block shape; the live run captured text only | 6.4 frames (deltas) | `live-check.md`; `@earendil-works/pi-ai/dist/types.d.ts` |
| O21 | The `['a']` append paths of tool-call arguments in the live message (`content[i].arguments` or `content[i].partialJson`) and how a live ToolItem's `args` are completed before its entry arrives; the live run had no tool call | 6.4 frames (L2) | `live-check.md` |
| O22 | The host `history` command on the wire: the newest page first without `before`, entries in descending id order, `next` opaque and absent on the last page | 6.4 intro, backend (cache and window, gap fill) | `docs/contract.md` §6.17 (read); `pi-durable/dist/harness/transcript.js` (read); not run |
| O23 | The window past five entries: its 50 and 100 bounds, the splice of its oldest entries and the `before` it sets, and a `view` frame after the first (a rebase) from a live host | 6.4 frames and backend | `pi-durable/dist/harness/view.d.ts` (read); `live-check.md` (five entries) |
| O24 | `pi.usage` keeps the cost of turns that a compaction removed (cumulative per model): the live run showed the growth of a model's record over two runs; a compaction was not run | 6.4 facts | `live-check.md` |
| O25 | Whether `busy` (`docs['pi.live'].run`) matches `host_state.state` while a dialog is held and the run waits on a `ui_request` | 6.4 backend (status) | not run |
| O26 | Live output of `cpd cockpit serve` (not run): the frame shapes and the stdin EOF exit 0 are read from `docs/commands.md` §11a and `docs/contract.md` §5.5 on `origin/main`; the stdin EOF exit (13.1), the frame shapes (13.8) and the unverified limits (13.10) rest on them | 13.1, 13.8, 13.10 | commands.md §11a (read, not run) |

Counting rule: `grep -c 'unverified' web/CONTRACT.md` counts lines mentioning the word; each line either states one of O1-O26 inline or is this section.

## 10. Phase 2 (pro-d20)

Normative. Scope decision pro-d20 (User, 2026-11-03 EST), Plan `webui-phase2-fast`. It fixes the exact seams of Phase 2 and supersedes the older statements marked "superseded by 10.x" in sections 1 to 9. Phase 2 touches only `web/**`; `web/package.json` and `pnpm-lock.yaml` stay byte-identical (pro-d17; `cpd:pi-1-5-upgrade` edits them). Visual polish is Phase 3/4 (pro-d16, pro-d19). Survey artefacts cited: `$CPD_HOME/run/webui-phase2-fast/explore/scout-web.md` (Phase 1 code map), `scout-v2shape.md` (durable v2 frames), `scout-dur.md`.

Accepted assumptions of Phase 2:

| Id | Assumption |
|---|---|
| P2-A1 | Transcript virtualization is hand-rolled: pure module `web/src/ui/chat/virtual.ts` plus a `ResizeObserver` boundary (`SizeObserver`); no new dependency. |
| P2-A2 | durable-host protocol v2 (`view`/`ops` frames, pi-durable 1.5.0) exists only on the unmerged branch `origin/pi-1-5-upgrade/state`. web/ supports v1 and v2 as two dialects chosen by `welcome.protocol`; v2 is tested only against mocked frames; v2 `history` paging beyond the host's newest-50 view window is out of scope. |
| P2-A3 | Every `cpd` session-message with `details.data.from === null` is shown as a User message (the engine also writes from-null nudges; known imprecision, a stated limitation), and durable `cpd-chat` entries (what the host writes for web-typed text) are user items. |
| P2-A4 | Every other `display: true` `cpd` custom_message (session-message from a session, job-merged, plan-done, escalation, ...) is a collapsed fold of fold kind `cpd`; `cpd-error` stays dropped. |
| P2-A5 | A sent message stays visible as a pending row until a matching user item arrives, a tail window replaces the transcript, or 30 s pass. |
| P2-A6 | Only `web/**` is touched; add only the structural CSS that makes the panes scroll. |
| P2-A7 | Streaming markdown without a new dependency: text is split into top-level segments and each segment is a memoized renderer, so only the last open segment re-parses per token. |
| P2-A8 | Out of scope, stated as Phase 2 limitations: keep-mounted rows for selection, in-app search, persisting fold open state across unmount, v2 `history` paging. |

### 10.1 cpd messages in the pi transcript (`web/src/server/pi/jsonl.ts`, `web/src/protocol/domain.ts`)

A `custom_message` with `customType` `"cpd"` and `display !== false`:

- (a) `details.kind === "session-message"` and `details.data.from === null` and `details.data.text` a non-empty string -> `UserItem` with id `u:<entry.id>`, `text` = `details.data.text`, `at` = `details.at` (else `entry.timestamp`), `attachments` `[]`, and `facts.lastRole = "user"`.
- (b) every other such entry -> `FoldItem` with id `f:<entry.id>`, `foldKind` `"cpd"`, `defaultOpen` false, `body` = the text of `content`, and `title`: for a session-message with `data.from` a string, `Message from <data.from>`; otherwise the first line of `details.text` (else of `body`) cut to `FOLD_TITLE_MAX`.

`FoldKind` gains `"cpd"` (3.3). `countItems` counts each of these entries as 1 (window sizing). An entry without an id is a parse error (`malformed_line`) like the other custom messages. `customType` `"cpd-error"`, `"cpd-first-message"`, `"cpd-reminder"` and `display === false` stay dropped.

Stated limitation (P2-A3): the engine also writes from-null nudges (messages with `data.from === null` that are not typed by the User); they show as User messages. Imprecise by design, not fixed in Phase 2.

### 10.2 Durable user messages

In both dialects an entry of kind `"cpd-chat"` (the host writes it for text typed in a web chat: `pi/durable-host/run.ts` submit with entry kind `cpd-chat`) is normalised exactly like kind `"pi.user"` (`UserItem` id `u:<entry.id>`). Entry kinds `cpd-event`, `cpd-first-message`, `cpd-error`, `cpd-plan-review` stay dropped (limitation).

### 10.3 State layer (`web/src/state/*`)

1. **Amended by 16.1 (air-d44):** `PendingSubmission.state` is `"sending" | "sent"`, never `"failed"`: a `session.error` carrying a `clientSubmissionId` drops its pending row and shows the error once in the cockpit error toast (11.7) instead of marking the row. `session.submitAck` with `queued: false` sets that pending row to `"sent"` (it is no longer removed); with `queued: true` the row is removed as in Phase 1 (the host's queued list shows it). A `"sent"` row is removed when a `session.append`, `session.update` or `session.window` carries a NEW user item (id not yet in the transcript) whose text, trimmed, equals the row's text, trimmed: oldest matching `"sent"` row first, one row per item. A `session.window` with `mode: "tail"` removes all `"sent"` rows. `connection.ts` schedules `scheduler.after(PENDING_ECHO_MS, ...)` at the ack (`PENDING_ECHO_MS = 30000`, exported) that dispatches `dismissPending` for the row; the timer is disposed at `close`.
2. `TranscriptState` gains `tailEpoch: number` (`emptyTranscript` has 0): `setTail(window, previous?)` returns `(previous?.tailEpoch ?? 0) + 1`; `prependEarlier`, `appendItems`, `updateItems` and `applyDelta` keep it.
3. `applyDelta` locates its item searching from the END of the list (the streaming item is last) and copies only the changed item; every other item keeps reference identity. `mergeById` uses one id->index `Map`.
4. Frame coalescing: `StoreAction` gains `{type: "serverBatch"; messages: ServerMessage[]}` which reduces the messages in order and produces ONE new state (one listener notification). `createConnection` buffers messages of type `session.append`, `session.update` and `session.delta` whose `sessionId` is the selected session; the first buffered message arms `scheduler.after(FRAME_MS, flush)` (`FRAME_MS = 16`, exported); `flush` dispatches one `serverBatch`. Adjacent buffered `session.delta` messages with the same `sessionId`, `itemId`, `blockIndex` and `blockType` are merged by concatenating `text`. Any other server message flushes the buffer synchronously first (order is preserved) and is then dispatched as before. `selectSession` and `close` drop the buffer and dispose the timer. A parse failure of a buffered message surfaces as `session.error` exactly as in Phase 1 (parse happens before buffering).

### 10.4 Transcript view (`web/src/ui/chat/Transcript.tsx`, `rows.ts`, `virtual.ts`)

**`web/src/ui/chat/rows.ts`** (pure):

```ts
export type Row =
  | { kind: "item"; key: string; item: TranscriptItem }
  | { kind: "tools"; key: string; tools: readonly ToolItem[] };
export function groupRows(items: readonly TranscriptItem[]): Row[];
```

A run of two or more consecutive tool items is one `tools` row keyed `group:<first tool id>`; a lone tool item is an `item` row keyed by its id; every other item is an `item` row keyed by `item.id`.

**`web/src/ui/chat/virtual.ts`** (pure) exports:

```ts
export const ESTIMATED_ROW_PX = 96; export const OVERSCAN_PX = 400; export const PIN_THRESHOLD_PX = 48; export const DEFAULT_VIEWPORT_PX = 800;
export interface Layout { keys: readonly string[]; offsets: readonly number[] /* length keys.length + 1, offsets[0] = 0 */; total: number }
export function layoutRows(keys: readonly string[], heights: ReadonlyMap<string, number>, estimate = ESTIMATED_ROW_PX): Layout; // measured height when known, else estimate
export function visibleRange(layout: Layout, scrollTop: number, viewport: number, overscanPx = OVERSCAN_PX): { start: number; end: number };
export interface Anchor { key: string; intra: number }
export function findAnchor(layout: Layout, scrollTop: number): Anchor | null;
export function scrollTopForAnchor(layout: Layout, anchor: Anchor): number | null;
export interface Metrics { scrollTop: number; clientHeight: number; scrollHeight: number }
export function isNearBottom(m: Metrics, threshold = PIN_THRESHOLD_PX): boolean;
export interface SizeObserver { observe(el: Element, onHeight: (height: number) => void): Disposable } // Disposable from protocol/runtime.ts
export function browserSizes(): SizeObserver;
```

- `visibleRange`: `end` is exclusive; the range holds the rows whose `[offset, offset + height)` intersects `[scrollTop - overscanPx, scrollTop + viewport + overscanPx]`; `scrollTop` is clamped to `[0, total]`; an empty layout gives `{start: 0, end: 0}`.
- `findAnchor`: the row containing `scrollTop`, `intra = scrollTop - its offset`; `null` for an empty layout. `scrollTopForAnchor`: the offset of the anchor key plus `intra`; `null` when the key is absent.
- `isNearBottom`: `scrollHeight - scrollTop - clientHeight <= threshold`.
- `SizeObserver.observe`: `onHeight` gets the element's border-box height; the callback fires on observe and on every change. `browserSizes()` is `ResizeObserver` based; when the global `ResizeObserver` is undefined it returns an observer whose `observe` is a no-op `Disposable`, so `DEFAULT_VIEWPORT_PX` and the estimates apply.
- Documented bound: a 1000-row layout at viewport <= 1600 px with no measured heights mounts at most 60 rows (`MAX_MOUNTED_ROWS` is not a runtime cap).

**`Transcript` props:** `{items, hasEarlier, queued, pending, onLoadEarlier(), tailEpoch: number, sizes?: SizeObserver}` with `sizes` defaulting to `browserSizes()`; `PendingSubmission` is imported from `state/store.ts` (no local copy).

**DOM.** `<div data-part="transcript">` wraps `<div role="log" aria-label="Transcript" data-at-bottom="true|false">` and, after it, `<button type="button" data-action="jump-to-bottom">Jump to bottom</button>` rendered only while not at bottom. Inside the log, in order: the Load earlier button (when `hasEarlier`); `<div data-part="virtual-space" style="height:<total>px; position:relative">` holding `<div data-part="virtual-window" style="transform:translateY(<offset of first mounted row>px)">` with one `<div data-row-key="<row key>">` per mounted row (the Phase 1 row markup inside); then the `<ul aria-label="Queued messages">` (queued + pending rows) when non-empty. All offset math uses `scrollTop` relative to the top of `virtual-space` (the Load earlier button above it is excluded).

**Heights.** A `Map<rowKey, number>` in a ref, filled by `sizes.observe(rowElement, ...)` for every mounted row; an update below 1 px of difference is ignored; a change re-lays out. The log element is observed too and gives the viewport height (`DEFAULT_VIEWPORT_PX` until observed).

**Sticky bottom.** `pinned` starts true. On every scroll event of the log `pinned = isNearBottom(log metrics)` and `data-at-bottom` mirrors it. After each commit (layout effect) while pinned the log's `scrollTop` is set to its `scrollHeight`. Pin (and scroll) also happens when `tailEpoch` changes, when `pending.length` increases, and on a click of Jump to bottom. While not pinned, a new item, a streaming change or a height change at or below the viewport does not change `scrollTop`.

**Anchor on prepend.** When the first row key changes, the previous first row key still exists in the new rows and `tailEpoch` is unchanged, `scrollTop` is restored with `scrollTopForAnchor` from the `Anchor` found before the commit (Load earlier never moves the visible row). While not pinned, when the measured height of a row whose bottom edge is above the viewport top changes, `scrollTop` changes by the same delta.

**Memoization.** Row components are memoized (`React.memo`): `Item`, `ToolRow`, `OrphanRow`, `FoldRow`, `CompactionRow` and the tools-row component (compared by its `tools` array elementwise), so a change to one item re-renders only its row (rows are keyed by `Row.key`). This supersedes the `tool-group` `<details>` wrapper of 6.8: a `tools` row renders its `ToolRow`s directly under the row's `data-row-key` div.

### 10.5 Structural CSS (`web/src/ui/app.css`, `Shell.tsx`)

- The shell grid fills 100vh with `overflow: hidden`, `grid-template-rows: auto minmax(0, 1fr) auto` (header, body, status bar; rows that are absent are dropped) and every grid child has `min-height: 0`.
- The Context tree column, the Plan pane section and the Inbox column each clip (`overflow: hidden`, pro-d69, A9, A10) and never scroll themselves: the Plan pane content, the Inbox list and the Sidebar sections scroll through `ScrollArea` (DESIGN 3c). `ScrollArea` is the base-ui wrapper: `web/src/ui/components/scroll-area.tsx` over `@base-ui/react/scroll-area` (exact pin 1.9.0 in `web/package.json`, pro-d72); `radix-ui` stays for the other primitives. The chat transcript's scroll element is the `ScrollArea` Viewport composed with the MessageScroller viewport (DESIGN-chat 2.2); the AskCard, the PlanReviewCard body and every `DialogContent` scroll through `ScrollArea` too (DESIGN 3c). The Plan pane's share of the centre is its resizable Panel (`maxSize` 80%), not a `max-height`. `ScrollArea` renders a vertical and a horizontal bar and wraps its children in `Content` (`data-slot="scroll-area-content"`, pro-d72); the vertical-only panes constrain the content width with the class pair of DESIGN 3c. When the Plan pane is collapsed, its body and its scrollbars are hidden (`app.css`, DESIGN-panes 1j, pro-d76). The center column is a flex column with `min-height: 0`.
- `[data-chat-dropzone]` is a flex column, 100% height, `min-height: 0`, `overflow: hidden`.
- `[data-part="transcript"]` is `position: relative; flex: 1; min-height: 0; display: flex; flex-direction: column`; the transcript's scroll element (the `ScrollArea` Viewport composed with the MessageScroller viewport, pro-d72) is `flex: 1; min-height: 0`; the Jump to bottom button is `position: absolute` at the bottom centre of `[data-part="transcript"]`.
- No colours, fonts or spacing beyond this (Phase 3/4).

### 10.6 Streaming markdown (`web/src/ui/chat/Markdown.tsx`)

`splitMarkdown(text: string): string[]` splits at blank lines (runs of two or more newlines) that are outside a fenced code block (` ``` ` or `~~~`); a fence that never closes keeps the rest of the text in the last segment; text with no blank line is one segment; segments joined with `"\n\n"` re-form the text modulo extra blank lines; empty text -> `[]`.

`Markdown({text})` renders each segment with a memoized component (`React.memo` compared by its text string, keyed by segment index) that wraps `react-markdown` with `remark-gfm`. Assistant text blocks, `FoldRow` and `CompactionRow` render through `Markdown`. Limitation: reference-style link definitions in another segment do not resolve.

### 10.7 App (`web/src/ui/App.tsx`)

`App` subscribes with separate selectors (`cockpit`, `session`, `selectedSessionId`) instead of the whole state. `ContextTree`, `PlanView` and `Inbox` are used through module-level `memo(...)` wrappers with props that are stable between renders (handlers from `useCallback` bound to the connection), so streaming transcript updates do not re-render them.

### 10.8 Durable protocol v1 and v2 (`web/src/server/durable/{client,frames,ops,backend}.ts`)

- Hello is `{type: "hello", protocol: 2, min_protocol: 1, client: "cpd-web"}`; the host answers with the negotiated `welcome.protocol` (a v1 host picks 1, a v2 host 2).
- `assertSupportedHost` accepts (protocol 1, `pi_durable` "1.3.0") and (protocol 2, `pi_durable` "1.5.0") only, else `unsupported_host_version`.
- `HostFrame` gains `{type: "view"; epoch; seq; view: unknown}` and `{type: "ops"; epoch; seq; ops: unknown[]}`; `attached.mode` is `"snapshot" | "view" | "resume"`.
- New pure module `ops.ts`: `applyOps(root: unknown, ops: readonly unknown[]): unknown` implements the Chord ops `r`, `s`, `d`, `a`, `t`, `p`, `m` exactly as written in `$CPD_HOME/run/webui-phase2-fast/explore/scout-v2shape.md` sections a4 and b1 (immutable with structural sharing, all-or-nothing; refuses the path segments `__proto__`, `constructor`, `prototype`, out-of-range indices, wrong-type targets and unknown ops with `WebError` `protocol_error`). `createReplica(): Replica` with `view(frame)`, `ops(frame)`, `value: unknown | null` and `cursor: HostCursor | null`: a view frame replaces the value and sets the cursor; an ops frame needs a non-null value, the same epoch and `seq === cursor.seq + 1`, else `protocol_error` (the replica is unchanged when it throws).
- The `Dialect` interface becomes `{readonly transcript; applyBase(frame: {type: "snapshot"; snapshot: unknown} | {type: "view"; epoch: string; seq: number; view: unknown}); applyIncrement(frame: {type: "events"; events: unknown[]} | {type: "ops"; epoch: string; seq: number; ops: unknown[]}): DialectOutput}`; `createDialect(protocol: 1 | 2): Dialect` (v1 behaviour byte-identical to Phase 1; the old `applySnapshot`/`applyEvents` are renamed).
- The v2 dialect follows `scout-v2shape.md` sections b and c: committed entries are folded from `replica.entries` by id (known ids are never re-normalised except a tool item gaining its result); the live assistant item comes from `docs["pi.live"].generation.message` (id `live:<epoch>:<n>`, replaced under the same item id by the committed `a:<entry id>` when the pass that adds the entry removes the live message); `busy` = `docs["pi.live"].run` is present; `queued` = `docs["pi.inbox"].items` mapped to `{id, mode}`; footer facts: model/thinking from `docs["pi.agent"]`, cost from `docs["pi.usage"]` models+tools `cost.total`, last context tokens from the last `pi.assistant` `usage.totalTokens`; a present-but-malformed doc is `protocol_error`, an absent doc is `unknown` as in v1. Window drops (`["p", ["entries"], 0, n, []]`) never delete known items.
- The backend picks `createDialect(client.welcome.protocol)` and routes `snapshot` | `view` to `applyBase` and `events` | `ops` to `applyIncrement`.
- Supersedes 6.4 where it fixes protocol 2 only (hello `min_protocol: 2`, `SUPPORTED_PI_DURABLE`, `applyView`/`applyOps` on `Dialect`, the `applyImmutable` dependency of A2, the single replica/cache path); the v2 cache-and-window and `history` gap behaviour of 6.4 is unchanged but untested against a real host (unverified, see 10.9).

### 10.9 Type single-sourcing and Phase 2 limitations

Canonical homes (each declared exactly once under `web/src`, imported elsewhere):

| Type | Home |
|---|---|
| `Cpd`, `CpdExec`, `CpdResult` | `server/cockpit/cli.ts` |
| `GitBoundary` | `protocol/runtime.ts` |
| `ResolvedSession` | `server/hub/registry.ts` |
| `SessionEmit`, `SessionHandle`, `SessionBackend` | `server/hub/connection.ts` |
| `CockpitUpdate`, `CockpitSnapshot` | `server/cockpit/events.ts` |
| `PendingSubmission` | `state/store.ts` |

`CockpitService` stays declared in both places on purpose: the hub declares the narrower consumer interface. `parseClientMessage` moves to `protocol/wire.ts` and `parseCockpitAction` to `protocol/cockpit.ts` (their `WebError` codes unchanged: `invalid_message`, `unknown_action`); `server/hub/connection.ts` and `server/cockpit/actions.ts` import them.

Phase 2 limitations (P2-A8): no keep-mounted rows for selection (selected text in an unmounted row is lost); no in-app search; fold open state is not persisted across unmount; no v2 `history` paging beyond the host's newest-50 view window; reference-style link definitions across Markdown segments do not resolve (10.6); from-null `cpd` nudges show as User messages (10.1); durable entry kinds `cpd-event`, `cpd-first-message`, `cpd-error`, `cpd-plan-review` stay dropped (10.2).

Unverified without a running v2 host (unverified, tested only against mocked frames): tool-call streaming ops, multi-round tool runs, window overflow ops, history paging, `host_state.name` as title.

---

## 11. Amendments pro-d48..pro-d61 (wire, store, server)

Recorded decisions: pro-d48 to pro-d61, approved by the User (preview `cpd-webui`). Sources read: the approved preview snapshot (`/tmp/cpd-webui-consolidate-main/preview-snapshot`), its patch, and `origin/main`. Where this section and an earlier section differ, this section stands (the earlier text is kept for its history and is corrected in place where it is wrong today).

### 11.1 Cockpit actions and read model

- **`context.retire`** (new `CockpitAction`): `{ kind: "context.retire"; contextId: string; approve: string; reject: string }`. `approve` and `reject` are comma-separated record ids, either may be empty. `actions.ts` argv: `["context","retire","--context=<contextId>"]`, plus `--approve=<approve>` when non-empty and `--reject=<reject>` when non-empty. The parser requires `contextId` (the other two fields are strings).
- **Curation records.** `InboxRow.curation?: readonly CurationRecord[]`, `CurationRecord { id: string; text: string; detail: string }`. For each `curate-retire` row with a `contextId`, the read model runs `cpd context curation --context=<id> --limit=10` (a failure is ignored: the row has no `curation`). It reads the reply's `retire[]` entries `{ record, text, reason, target }`: `id` = `record`, `text` = `text`, `detail` = `reason` and `target` joined by a space (empty parts dropped). Entries without string `record` and `text` are skipped.
- **Host name.** `ContextTreeModel.hostname?: string`: the machine hostname, with a trailing `.local` removed; `host` when that is empty. Absent before the first tree read.
- **Session resolution of an Inbox row** (`readmodel.ts` `resolveSessionId`): the row's own `session`; else the Plan's `lead_session`; else the live (not archived) session with `plan_lead` of that Plan; else, for `question` and `curate-retire` rows with a Context, the live Manager of that Context (role `manager`, no Plan). `null` when none exists.
- **Lead rows.** A `pr-ready` row of red or yellow urgency counts as a Lead row for the tree (`LEAD_KINDS`, `readmodel.ts`).
- **Plan counts.** `PlanCounts.merging` is a number in the read model (absent reads as 0).

### 11.2 Store and connection (`web/src/state/store.ts`, `connection.ts`)

- State: `pendingActionIds: readonly string[]` and `completedActionIds: readonly string[]`, both `[]` at start.
- `actionStart {actionId}` adds the id to `pendingActionIds` (once). `connection.action(action)` dispatches `actionStart`, sends `cockpit.action {actionId, action}` and returns the `actionId` (a fresh id per call).
- `action.result` removes the id from `pendingActionIds`. With `ok: true` it adds the id to `completedActionIds` (once). With `ok: false` it sets `lastError` to the wire error.
- `dismissError` clears `lastError` and `cockpit.error`.
- `submitPending` stores the submission with `at` (the given `at`, else the current time as ISO) and with its `images` (`WireImage[]`), so the optimistic row can show thumbnails (DESIGN-chat 2.3.1, 2.7).
- `session.submitAck` for a non-queued submission marks the pending row `sent`, or removes it when the echoed user item is already in the transcript (matched on normalised text).
- Matching: `normalizeUserText(text)` drops a leading `Message from <name>:` prefix and any trailing `[Attached image: …]` lines, then trims. A user item replaces the oldest pending row (state `sending` or `sent`) whose normalised text is equal.

### 11.3 Images on submissions and the pi transcript

- **Send.** A pi `session.submit` with `images` no longer fails. The backend writes each image to the attachments directory (`<tmpdir>/cpd-webui-attachments/`, `web/README.md`) as `<sessionId>-<stamp>-<i>.<ext>` (`png`, `jpg`, `gif`, `webp` by MIME type) and appends one `[Attached image: <path>]` line per image after a blank line to the text it sends with `cpd session send`. `PI_SEND_TEXT_MAX` applies to that text. **Amended by 16.2 (air-d43):** with no text, the text sent is the `[Attached image: <path>]` lines alone, with no leading blank line; the message is delivered like any other. `capabilities.images` for pi is `{supported: true, maxCount: IMAGE_MAX_COUNT, maxTotalBase64: IMAGE_MAX_TOTAL_BASE64}`.
- **Echo.** The pi normaliser (`jsonl.ts`) reads each `[Attached image: <path>]` line of a user message: the line is removed from `text`, and the path becomes a `TranscriptAttachment {kind: "image", mimeType, dataBase64}`. `mimeType` from the extension (`png`, `jpg`/`jpeg`, `gif`, `webp`; default `image/png`). `dataBase64` is the file's base64 when it can be read; otherwise the attachment has no `dataBase64` and shows as a named attachment. Block images of the message (`ImageContent`) are kept as before, before the path images.
- Durable sessions: `images` as in 6.9 and 6.4, unchanged.

### 11.4 Interactive dialogs

- **pi `ask_user_question`.** The normaliser opens `call:<callId>` with `interactive: true` (was `false`). `respond({method:"ask_user_question", outcome})`: a `session.dialogResolved` (`answered` or `cancelled`) goes out at once; the response goes to `<cpdHome>/run/ask/<sessionId>.json` as `{dialogId, result}` (`result` = the answer, or `{answers: [], cancelled: true}` on cancel). The pi extension `pi/ask-user-question/ask-user-question.ts` polls that file (every 120 ms when `CPD_SESSION` is set), reads it once, deletes it and resolves the questionnaire; it deletes a stale file at its own start and end. If the file is still there 350 ms later, the backend deletes it, sends `Escape` to the session's tmux pane (`cpd session show`, field `tmux_pane`, when present), and for an answer posts the summary (one `question => value` line per answer, multi-select values joined by `, `) with `cpd session send`.
- **pi plan review.** The normaliser opens `review:<n>` with `interactive: true` for a `cpd-plan-review` entry with `status: "pending"`. `approve` and `reject` run `cpd plan decide <n> --approve|--reject --plan <planKey>`. `changes` writes the trimmed feedback to `<cpdHome>/run/<planKey>/feedback-<n>.md` and runs `cpd plan decide <n> --reject --feedback-file <path> --plan <planKey>`. `cancelled` sends nothing and resolves nothing.
- **durable plan review.** `frames.ts` reads `cpd-plan-review` entries (`kind: "review"`) and keeps the pending reviews; the dialect's `openDialog` is the latest pending review as `{id: "review:<n>", method: "cpd_plan_review", title, summary, interactive: true}` (title: the latest `cpd_plan_review` tool item's `args.title`, else `Plan review <n>`; summary: the latest `cpd-plan-summary` fold body). `backend.ts` `syncTranscriptDialog` opens it when no UI-held plan review exists, and resolves the transcript review dialogs that are no longer open. Responses: `approve`/`reject` run `plan decide` as above; `changes` runs `plan decide --reject` and then `client.submit({text: feedback, whenBusy: "followUp", requestId: "feedback:<dialogId>"})`; `cancelled` resolves `cancelled` without a call.
- **Capabilities.** `dialogs` is `{mode: "interactive"}` for pi (was `readonly`, A4). The read-only path of the dialog cards stays for any dialog with `interactive: false`.

### 11.5 Chat and composer contract changes (for the wire)

- The Composer sends `whenBusy: "followUp"` only (pro-d57). `steer` stays a valid wire value (durable) and stays unsupported for pi.
- No new server message type. `session.dialog` and `session.dialogResolved` carry the pi and durable dialogs of 11.4.

### 11.6 Cockpit UI facts the wire depends on

- The Inbox row's left click and its `Start`, `Merge`, `Post`, `RePost`, `Approve`, `Claim` and `Dismiss` buttons go through `onAction` and the store ids of 11.2 (DESIGN-panes 2.6, 2.7).
- The Header's breadcrumb is built from the tree, the plan and the selection (DESIGN-panes 4.1). A crumb click calls `connection.selectPlan` and `connection.selectSession`, which send `cockpit.selectPlan` and `session.attach` (6.2).

### 11.7 Errors

- An action failure shows the error code and message in the cockpit error Alert (DESIGN-panes 4.8). Its dismiss button dispatches `dismissError`.
- **Report button** (pro-d452; assumptions A4, A5). The error toast has a button, `data-part="error-toast-report"`, labelled `Send to <target>`, where `<target>` is `routeErrorTarget(error)`. A click dispatches `{kind: "error.report", target, code, message}` (the action of 5.4, argv in its table), with the toast's `code` and `message`, then `dismissError`.
- `routeErrorTarget(error: WireError): "cpd" | "host"` lives in `web/src/ui/errorReport.ts`. It returns `"host"` when `error.code === "cpd_unavailable"`, or when `error.details.cpdCode` is one of `store_too_new`, `spawn_failed`, `adapter_missing`, `gh_failed`, `slack_auth`, `slack_not_configured`, `jira_auth`, `jira_not_configured`, `classifier_unreachable`, or `error.message` matches `/store_too_new|Upgrade cpd|unreachable|needs a sign-in/i`. It returns `"cpd"` for every other error: `protocol_error`, `cpd_malformed_json`, `invalid_message`, `unknown_action`, an application `cpd_failed`, and session errors.

### 11.8 Footer

- `Footer` (`web/src/ui/chat/Footer.tsx`) renders only the context card (`{footer, contextWindow?}`, DESIGN-compose 3). The session telemetry row (cwd, session key, tokens, cache, model, tier, thinking, cost, branch) is removed from the chat (pro-d57). Its data stays in `SessionView.footer` for the composer's tier selector (DESIGN-compose 2).

### 11.9 The session-message origin (Plan ui-origin)

Recorded decision: Plan ui-origin (air-d23, air-d24; assumptions A2, A5). Where this subsection and an earlier section differ, this subsection stands. The `session-message` Event (docs/contract.md §5.2) gains the optional `data.origin` field, whose value is exactly `tui` or `webui` — the surface the message was submitted from. Absent means no origin: engine, agent and CLI `session send` without the flag leave it absent; it is set only by the WebUI paths.

- **The wire gains no field.** `session.submit` (4.1) carries no `origin`; the durable socket submit frame (6.4) carries none either, and is unchanged: the durable host's web submissions are `cpd-chat` entries, not `session-message` Events. No parser (4.3) learns a new field.
- **The pi runner sets it.** The pi backend's `submit` (6.6) passes `--origin webui` to `cpd session send`, so the `session-message` Event it writes carries `data.origin: "webui"`. The pi runner is the only WebUI path that writes a `session-message`.
- **The adapter reads it.** The pi adapter reads `data.origin` to tag a User message with its surface (the tag Job of the Plan); the field the adapter reads raises `protocol`, 23 → 24, in `core/src/events/kind.rs` and `pi/lib/event.ts` together.

---

## 12. Live pi stream: tokens and tool calls

Normative for `web/src/server/pi/live.ts` (new), `web/src/server/pi/backend.ts`, `web/src/server/pi/jsonl.ts`, `web/src/protocol/wire.ts` and `web/src/state/{transcript,store,connection}.ts`. The frames are the pi live-stream socket of `docs/contract.md` §6.21 (protocol 1, written by `pi/lib/live-stream.ts`). **Why:** pi writes the `.jsonl` only when a message ends (§6.21 "Why"), so a pi Session shows no tokens until then. **Sources:** `scout-8.md` (the seams), `scout-6.md` (match keys), `docs/contract.md` §6.21.

"Job assumption A6" below is the assumption of this section. It is not §1.3's A6.

### 12.1 Decisions

| Id | Decision |
|---|---|
| L1 | **Wire `session.remove`.** New server message `{type: "session.remove"; sessionId: string; itemId: string}`, the shape of `session.update` (`wire.ts:50`) with one `itemId` in place of `items`. Parsed like `session.update` (`connection.ts:53-55`): `sessionId` and `itemId` are strings. Client reducer `removeItems(state, itemId)` drops the item with that id; an unknown id returns the state unchanged (the same reference). It is not batched: a buffered batch is flushed first (§10.3 rule 4), so order holds. |
| L2 | **`web/src/server/pi/live.ts`.** Connects to `${cpdHome}/run/pi/${sessionId}.sock` through an injected `connect`. Retries every `LIVE_RETRY_MS` = 1000 through the injected `Scheduler` while the socket is absent or after it closes. Stops on detach: closes the socket and disposes the timer. |
| L3 | **Frames become items.** The table of 12.3. |
| L4 | **Reconcile with the `.jsonl` tailer.** `jsonl.ts` reports the committed lines it applies; `backend.ts` removes a live item before its committed item is appended, and ignores frames of a committed message. Rules in 12.4. |
| L5 | **User and toolResult messages are not made live.** The `.jsonl` serves them. |
| L6 | **No socket, no change.** With no socket the backend behaves exactly as today: no live item, no `session.remove`, and no wire message for an absent socket. |

**Job assumption A6** (webpi-docs): pi live items are ephemeral. The assistant item `live:<message.timestamp>` (streaming true) and the tool item `tool:<toolCallId>` are replaced by the committed `.jsonl` line with no duplicate. The socket is retried every 1 s while it is absent. **Inferred, not verified at runtime:** the `message.timestamp` of a `message_end` object equals that of the committed line (`scout-6.md` §1c; `docs/contract.md` §6.21 "Match keys"; a `message_end` handler may replace the message). If it does not hold, reconcile never removes the live item (12.6, point 3).

### 12.2 Item ids

| Item | Id | Fields | Made by |
|---|---|---|---|
| live assistant | `live:<message.timestamp>` | `streaming` true while open, false after `message_end` when it has text or thinking or a non-empty `errorMessage`; `model` = `message.model` when present | `message_start` (assistant), or the `inflight` message |
| live tool | `tool:<toolCallId>` | the same id as the committed tool item of the call (`jsonl.ts:246`, `tool:<callId>`), so the committed item replaces it | `tool_start`, or an `inflight` tool |
| committed assistant | `a:<entry id>` (`jsonl.ts:239`) | `streaming` false | the `.jsonl` tailer |
| committed tool | `tool:<callId>` | | the `.jsonl` tailer |

- `live:<ts>` relies on `message.timestamp` being unique among the assistant messages of one Session (`scout-6.md` §4). Scout-6 saw no collision in three sampled Sessions; that is not a proof. A collision would merge two messages.
- The `live:` prefix does not clash with the durable `live:<epoch>:<n>` (§3.3), because one Session has one runner.
- `streaming` is true for a pi live item (amends §3.3).

### 12.3 Frame-to-wire table

Terms. `T` is the `message.timestamp` of the **open** message. The open message is set by a `message_start` of an assistant message or by the `inflight` message. A `delta` frame carries no timestamp, so it belongs to the open message. `blockIndex` counts the text and thinking blocks of the open message that are in `inflight` content or have received a `session.delta`; toolCall content is not a block (`domain.ts:53`). A block that streamed no delta has no index until its first delta, so an empty block that streams nothing takes no index. `counter` is the next unused block index of the open message: it starts at the block count of `inflight` content (0 after `message_start`). `result.text` is the text blocks of `content` joined with `"\n"` (`domain.ts:60`, `jsonl.ts` `textOf`).

| Frame | Condition | Wire message(s) | Effect on the open message |
|---|---|---|---|
| `hello` | `protocol` is 1 | none | a new connection starts with the `inflight` frame |
| `hello` | `protocol` is not 1 | `session.error` `protocol_error` (12.5) | the source stops |
| `inflight` | `message` is assistant and T is not committed | `session.append` `AssistantItem` `live:T`, `streaming: true`, `blocks` = its text and thinking blocks | open = T; `blockIndex` counter = its block count |
| `inflight` | `message` is `null` | none | none |
| `inflight` | each entry of `tools` | `session.append` `ToolItem` `tool:<toolCallId>` as `tool_start`, plus `tool_update` when `partialResult` is not `null` | none |
| `agent_start`, `agent_end` | | none | none |
| `message_start` | role assistant, T not committed | `session.append` `AssistantItem` `live:T`, `streaming: true`, `blocks: []`, `model` when present | open = T; counter = 0 |
| `message_start` | role assistant, T committed | none | open = T; its frames are ignored |
| `message_start` | role `user` or `toolResult` | none (L5) | none |
| `delta` | kind `text` or `thinking`, phase `start` | none | none: no block index is allocated |
| `delta` | kind `text` or `thinking`, phase `delta`, `delta` a string | `session.delta` `{itemId: live:T, blockIndex, blockType: kind, text: delta}` | a content index not yet mapped is mapped to counter, then counter + 1; a mapped index reuses its blockIndex. The `session.delta` carries that blockIndex, which equals the client's blocks.length at that moment, so it creates the block (line 508) |
| `delta` | kind `text` or `thinking`, phase `end` | none | none: no block index is allocated |
| `delta` | kind `toolcall`, any phase | none | none |
| `tool_start` | | `session.append` `ToolItem` `tool:<toolCallId>`: `status` running, `name` = `toolName`, `args`, `callId` | the name and args are kept for the later updates |
| `tool_update` | | `session.update` `ToolItem` with the kept name and args, `result` `{text, isError: false}`, `status` running | none |
| `tool_end` | | `session.update` `ToolItem` with the kept name and args, `result` `{text, isError}`, `status` `done` (isError false) or `failed` (isError true) | none |
| `message_end` | role assistant, T not committed, a text or thinking block in `message.content`, or a non-empty string `message.errorMessage` | `session.update` `AssistantItem` `live:T`: `blocks` = the final text and thinking blocks, then the text block `"Error: " + errorMessage` when `errorMessage` is a non-empty string, `streaming: false` | open cleared |
| `message_end` | role assistant, T not committed, no text or thinking block and no non-empty `message.errorMessage` | `session.remove` `live:T` (the rule of `domain.ts:53`) | open cleared |
| `message_end` | role assistant, T committed | none | open cleared |
| `message_end` | role `user` or `toolResult` | none | none |
| `.jsonl` tailer | assistant line with T, committed | `session.remove` `live:T`, then `session.append` of `a:<entry id>` when the line yields an `AssistantItem` (12.4, rule 3) | none (committed) |
| `.jsonl` tailer | toolResult line, `toolCallId` committed | the existing `session.update` of the tool item (`jsonl.ts:271-275`) | live frames of that `toolCallId` are ignored (12.4, rule 2) |

Note: `ToolItem.result` is set while `status` is running (a partial output from `tool_update` or `inflight`). `domain.ts:71` says "running = no result yet". This is the one departure from that comment, and the committed result replaces the partial output.

### 12.4 Reconcile rules

1. **Committed sets.** Per attach, the backend keeps `committedTurns` (the `message.timestamp` of each committed assistant line) and `committedCalls` (the `toolCallId` of each committed toolResult line). `jsonl.ts` reports both for every line it applies, in the tail read and in every tailer push (`PushResult`).
2. **Ignored frames.** A frame of a message T in `committedTurns` is ignored: `message_start`, `inflight` with that message, `message_end`, and the `delta` frames while T is open. A tool frame whose `toolCallId` is in `committedCalls` is ignored. Why: the committed tool item carries fields the live one lacks (the edit `diff`, `jsonl.ts:272`), and a full-replacement `session.update` from a late `tool_end` would drop them.
3. **Committed assistant line.** For a committed assistant line with T, the batch emits `session.remove` `live:T` before its `session.append`. It does so for every such line, including a tool-only one that yields no `a:` item (`domain.ts:53`). A remove of an absent id is a no-op (L1).
4. **Tool items.** A committed tool item has the id of the live one, so the append replaces it (`mergeById`, `transcript.ts:29-39`). A committed tool item is `running` until its toolResult line, so a live status set before that line is replaced by `running`, then by the result.
5. **Either order.** Live first, then committed: the live item is removed and the committed one takes its place. Committed first, then live: the live frames are ignored (rule 2). Neither order gives a duplicate.

### 12.5 Connection, errors and stop

- **Path.** `${deps.cpdHome}/run/pi/${sessionId}.sock`, with `sessionId` = `ref.id` (`backend.ts:80`). `CPD_SESSION` is the Session id (§6.6), and `pi/lib/host.ts:92` falls back to it for the Session row id. The pi side names its socket `${CPD_HOME}/run/pi/${CPD_SESSION}.sock` (`pi/lib/live-stream.ts:35`).
- **`connect`** is injected, as the durable backend's is (`durable/backend.ts:54`): `connect: (socketPath: string) => Promise<LiveSocket>`. It rejects when no socket listens. `LiveSocket` yields the lines of the socket and reports its close. The real one is built where the durable `connect` is built (`plugin.ts`).
- **Retry.** An absent socket (`connect` rejects) and a closed socket both schedule `scheduler.after(LIVE_RETRY_MS, retry)`. Neither writes a wire message.
- **Protocol errors.** A line that is not JSON, a frame whose `t` is not in 12.3, or a `hello` whose `protocol` is not 1 is a `protocol_error`. The source reports one `session.error` `{sessionId, error}` with `details: {line}`, closes the socket and stops for this attach. It does not retry, because a retry cannot read a frame the client does not know (the Job's decision; the error table of §7 gives `protocol_error` its `{field?, line?}`).
- **Stop.** Detach closes the socket and disposes the timer. Messages after dispose are dropped (`backend.ts:143-145`).
- **Read only.** The client writes nothing to the socket (`docs/contract.md` §6.21 "Safety").

### 12.6 Open points and limits (not decided here)

1. **Close while a message is open.** When the socket closes (pi exits or reloads, or the socket drops) the open live items stay as they are: an open message keeps `streaming: true` until a committed line replaces it. A message that never commits (pi killed mid-message) stays streaming. Not decided: remove the open live items on close, or set `streaming` false. The Lead decides.
2. **A tail window drops live items.** A `session.window` of mode `tail` (sent by `reload`, `backend.ts:197-204`) replaces the transcript (`setTail`, `transcript.ts:41`). The live items are gone. The later deltas of the open message find no item and are ignored (`applyDelta`, `transcript.ts:69`) until its committed line arrives. The live source does not re-send the open message after a window.
3. **Identity of `message.timestamp`** between the `message_end` object and the committed line (Job assumption A6, 12.1). If the identity fails, the live item of that message is never removed by reconcile.

## 13. Cockpit stream: `cpd cockpit serve` (Plan `cpd-realtime-engine`)

Normative. Plan `cpd-realtime-engine` (its Job `webcockpit-docs`) decides the five points below. Where a statement of 6.3 is marked "Amended by 13.x" or "superseded by 13.x", section 13 wins and the older statement is void.

Sources: `docs/commands.md` §11a and `docs/contract.md` §5.5 on `origin/main` (commit `665c0c159`). The frames, the requests and the exit behaviour are read there and are not run here (unverified, O26). The seams are in `$CPD_HOME/run/cpd-realtime-engine/explore/scout-7.md` (cited as scout-7).

Plan assumptions used here (Plan `cpd-realtime-engine`, not this file's A1 to A7): **PA1** one long-lived NDJSON process, stdin EOF exits 0; **PA2** snapshot panes equal the list commands' JSON, and snapshots are full; **PA4** the other per-request spawns stay (every `actions.ts` command, and each spawn of 6.3 that 13.6 does not replace); **PA5** the web server uses the stream while it is up and the spawn refresh while it is down or not yet ready, and the child restarts with backoff.

### 13.1 Decision 1: the child (`web/src/server/cockpit/stream.ts`)

- `stream.ts` owns exactly one long-lived child, `cpd cockpit serve` (no arguments; `cockpitServeArgs()` = `["cockpit","serve"]`). `stream.ts` spawns nothing. The spawn is `CockpitStreamBoundary.start(): StreamProcess` in `cli.ts`, the only module that runs `cpd` (2.1). `plugin.ts` builds the real boundary and tests fake it (A6). `StreamProcess` has the shape of `FollowProcess` (6.3) plus `write(line)` and `closeStdin()`.
- stdin is a pipe that stays open for the child's whole life. Requests are written to it as lines.
- stdout is split into lines as `events follow` is (`FollowProcess.onLine`, 6.3). stderr is logs only; its tail is kept for an exit message (13.9).
- `stop()` closes stdin, then kills the child (SIGTERM). Either alone ends it: stdin EOF exits 0 (PA1). A parent that dies closes its end of stdin, so the child never outlives its parent.
- `stream.ts` reports typed frames and the ready state. It reads no pane and builds no model; the mapping is in 13.8 and lives in `events.ts`.

### 13.2 Ready state

- The stream is **ready** from the child's `hello` until that child exits. Before `hello`, and between an exit and the next child's `hello`, it is not ready. Readiness belongs to one child: a new child is not ready until its own `hello`.

### 13.3 Decision 2: snapshot panes and `plan_show`

- The service takes the panes of a `snapshot` frame as the cockpit's panes. It keeps the parsed rows of the last applied snapshot: `contexts`, `plans`, `sessions`, `inbox`, and `cursor` (the minimum of the four `events_cursor`). The rows are parsed by the read model's own functions (`parseContextList`, `parsePlanList`, `parseSessionList`, `parseInboxList`, 6.3) and the tree and inbox are built by its own builders (`buildTree`, `buildInbox`). `events.ts` holds no copy of a parser or a builder.
- The plan view is not a snapshot pane (the `plan` pane of a snapshot is `cpd plan list`). It is read by a `plan_show` request when a Plan is selected, answered by a `plan_show` frame, then `parsePlanShow` and `buildPlanView` as in 6.3.
- A request carries a string id that the service chooses, unique within its child, and the plan id it asks for. A reply for a Plan that is no longer selected is dropped.

### 13.4 Decision 3: coalescing

- `COALESCE_MS` = 16 (one frame). It replaces 250 of 6.3 and has two uses: the spawn fallback's burst of 6.3 rule 3, and the emission of snapshot panes.
- A `snapshot` frame stores its rows at once. If no emit is pending, one is scheduled `COALESCE_MS` later. When it fires, the tree and then the inbox are built from the rows stored at that moment and emitted once. Snapshots are full (PA2), so the latest stored snapshot is enough.

### 13.5 Decision 4: the `session-state` patch

- An `event` frame of kind `session-state` with a non-null `session` changes the stored row of that session: its `state` becomes `data.to`. `data.to` must be one of the five states of 3.2; otherwise it is an error (13.8) and nothing changes.
- A session not among the stored rows is not patched. A Worker is not in the tree (scout-7 §4), and a session registered in the same batch is not stored yet.
- The tree is rebuilt by `buildTree` over the stored rows. The dot is therefore computed by the read model's own dot rule (5.1), with `attention` taken from the stored inbox rows. The tree pane is emitted in the same tick, with no coalescing wait. Target: under 5 ms (unverified, not measured).
- The next `snapshot` replaces the stored rows and so reconciles the patch.
- Only `session-state` patches the panes. Every other kind waits for the snapshot that follows its batch (5.5: a batch's `event` frames come before its snapshot).

### 13.6 Decision 5: fallback and readiness

- While the stream is not ready, the service runs 6.3 unchanged: the `events follow` child from `followArgs(lastCursor)`, the coalesced refresh of 13.4 through `loadPanes` (spawn), and the plan pane through `loadPanes`.
- The follow child runs while the stream is not ready. `start()` starts it as 6.3 says. The first `hello` stops it. A child's exit starts it again from `lastCursor`. `lastCursor` is the highest Event id seen from either source, so Events committed while the stream was down come back through `events follow --since`, and the next snapshot carries the state.
- `start()` still runs `loadAll` by spawn first (6.3 rule 1), then starts the stream.
- A `plan_show` request made while the stream is not ready, or in flight when the child exits, resolves `down` at once. The service then loads the plan by spawn.
- **`store_too_new` heal** (pro-d452; A3). An `error` frame with a null id and `code` `store_too_new`, or a `plan_show` rejection (an `error` frame with a string id) with `code` `store_too_new`, makes the stream kill the child and respawn it at once, with no wait of 13.7. It reports no `cockpit.error`. Its pending requests, the rejected one included, resolve `down` as for a restart. If the respawned child emits `store_too_new` again before its `hello`, that error is reported as an ordinary `cpd_failed` with `details.cpdCode` `store_too_new`, and the normal backoff of 13.7 applies; that child is not healed again.

### 13.7 Restart backoff

| Constant | Value | Meaning |
|---|---|---|
| `STREAM_RESTART_BASE_MS` | 500 ms | the first restart waits this long |
| `STREAM_RESTART_CAP_MS` | 10 000 ms | no restart waits longer |

- The restart after the n-th consecutive failure waits `min(STREAM_RESTART_BASE_MS * 2^n, STREAM_RESTART_CAP_MS)`, n counted from 0: 500, 1 000, 2 000, 4 000, 8 000, 10 000, 10 000 ms, and so on. A failure is a child that exits before its first snapshot is applied.
- n resets to 0 when a child's first snapshot is applied, not at `hello`: a child that says hello and dies at once keeps backing off. A repeated `hello` on the same child is not a new child: it is an in-place restart (13.9), and it changes neither n nor the wait.
- The first start of the process has no wait. Each wait is a Scheduler timer (A6); `stop()` cancels it.
- The `store_too_new` respawn of 13.6 is not a wait: it starts at once, and it does not change n.
- Unchanged for the fallback: `SAFETY_REFRESH_MS` = 30 000 and `FOLLOW_RESTART_BASE_MS` = 1 000 with its cap of 30 000 (6.3 rule 7).

### 13.8 Frame-to-pane mapping

Each line is parsed once, at the edge (`parseCockpitFrame`, `stream.ts`). A line that is not one JSON object, has no string `t`, or has fields its `t` does not take, is `cpd_malformed_json` (7).

| Frame (`t`) | Accepted shape | Effect on the cockpit |
|---|---|---|
| `hello` | `protocol` = 1 | Ready (13.2); the follow child stops (13.6). On a child that already said hello, it is an in-place restart (13.9): the stream stays ready. Any other `protocol`: `cpd_malformed_json` with `details.field` = `protocol`; the child is killed and the restart of 13.7 applies, while the spawn path serves the panes. |
| `snapshot` | `seq` an integer; `panes` with `context`, `plan`, `session` and `inbox` (**amended by 20:** and `status`, §20.1), each an object | Parse the four panes (13.3). On success, store the rows, reset n when this is the child's first (13.7), and schedule the emit (13.4). A pane that fails to parse, or whose `ok` is not `true`, rejects the whole snapshot with the `cpd_failed` that `Cpd.json` gives for the same reply (6.3): one error update, the previous rows kept, no emit. A snapshot also clears `lastError` when its `details.cpdCode` is `store_too_new`, as it already clears `cockpit.error`. |
| `event` | `event` is one object that `parseEventLine` (6.3) accepts | `lastCursor` = max id. Kind `session-state`: 13.5. If `panesFor` of the event puts the plan pane in the set for the selected Plan, the plan is due: its `plan_show` request is sent after the snapshot that ends the batch. Any other kind changes nothing by itself. |
| `plan_show` | `id` matches a pending request; `value` is an object | Resolve the request: `parsePlanShow`, `buildPlanView`, and emit the plan pane if that Plan is still selected. |
| `error` with `id` a string | `error.code`, `error.message` | Reject the pending request with `WebError("cpd_failed", message, {cpdCode: error.code})` (7). `selectPlan` keeps the previous plan and emits one error update (6.3). An `id` that matches no pending request: one error update. A `store_too_new` code: the heal of 13.6 instead, with no error update. |
| `error` with `id` null | `error.code`, `error.message` | One error update, `cpd_failed` with `cpdCode: error.code`. The child keeps serving unless it exits (13.9). A `store_too_new` code: the heal of 13.6 instead, with no error update. |
| anything else | none | `cpd_malformed_json`: one error update, the line skipped, the child kept (as commands.md §11a: a malformed line keeps the process serving). |

### 13.9 Failure rules

- A child that exits while the stream is running is not ready. Its pending requests resolve `down` (13.6), the follow child starts (13.6), and a restart follows after the wait of 13.7. An exit caused by `stop()` is not a failure and emits nothing.
- A second `hello` on the same child is not an exit: `cpd cockpit serve` exec'd in place when its executable was replaced (commands.md §11a, pro-d450). Its pending requests resolve `down` (13.6), the stream stays ready, and `onError` is not called. No follow child starts and no restart wait applies.
- An exit emits one error update, unless an `error` frame with a null id has already reported it: `cpd_failed` with `cpdCode` `no_json` and the message the trimmed stderr, or `cpd exited <n>` when stderr is empty (the mapping of 6.3 for a non-JSON failure). A null-id `error` frame is followed by the exit of a store error (commands.md §11a), and that frame reports it.
- A failed write to stdin means the child is gone: it is killed and handled as an exit.
- A malformed frame never restarts the child (13.8).
- A child killed by the `store_too_new` heal (13.6) is a respawn, not a failure: its exit emits nothing, as the exit of `stop()` does, and its pending requests resolve `down`.

### 13.10 Tests and limits

- Tests run without a real process, socket, timer or `cpd` (A6). `web/src/server/cockpit/stream.test.ts` (new; fake `StreamProcess`, fake Scheduler): ready only after `hello`; the backoff sequence of 13.7 and its reset at the first snapshot; `stop()` closes stdin, kills the child, cancels the wait and emits nothing afterwards; a request is answered, rejected, or resolved `down` when not ready and when the child exits. `web/src/server/cockpit/events.test.ts` (changed): the 16 ms coalescing of snapshot panes and of the spawn burst; the panes of a snapshot equal the spawn path's panes for the same replies; a `session-state` patch emits the tree in the same tick and the next snapshot replaces it; the follow child runs only while the stream is not ready.
- Limits, stated plainly: the live output of `cpd cockpit serve` is unverified (O26); the 5 ms target is unmeasured; `seq` is informational, because one pipe keeps frames in order; no `hello` timeout is defined, so a child that never says `hello` stays not ready, with the spawn path serving the panes and no restart (unverified, O26).
  - **Amended by 15.2 (Plan `webui-ws`): `seq` is no longer only informational** — the serve-stream watchdog compares it (a `seq` that stops advancing is one of its two stall signals, §15.2). One pipe still keeps frames in order; the watchdog reads `seq`, it does not reorder anything.

## 14. Amendments air-n13, air-n15, air-d28: the Context tree's live refresh and the Plan pane's empty state (Plan `toast-fix`)

Normative. Plan `toast-fix` decides the two points below after the WebUI bugs air-n13/air-n15 (the Context tree froze at ~13:20 EST and never refreshed while `cpd cockpit serve` stayed alive) and air-n14 (with a Lead selected, the Plan pane rendered the flowchart instead of that Lead's Plan). Where a statement of 6.3 or 6.11 is marked "Amended by 14.x", section 14 wins and the older statement is void.

### 14.1 The Context tree refreshes live on every event batch (air-n13, air-n15)

- The tree pane refreshes on **every event batch from the cockpit stream**, while the stream is ready. `cpd cockpit serve` closes every committed batch with a fresh Snapshot whose panes are re-read (`docs/commands.md` §11a), so the service builds the tree pane from that Snapshot's rows (13.3, 13.4) and emits it for **every batch**, whether or not one of the batch's Event kinds is in the tree list of 5.5 (`panesFor`). While the stream is ready the tree never depends on a timer: the safety refresh of 6.3 rule 6 keeps its not-ready role (13.6) and is not the tree's refresh.
- **A lower tree cursor is not a stale tree.** `ContextTreeModel.cursor` is the minimum of the four commands' `events_cursor`s (5.1), so a batch that advances a pane whose cursor was above the minimum makes that minimum go down: the tree of a newer batch can carry a cursor lower than the held tree's, and the guard of 6.3 rule 11 would drop a newer tree for an older-looking one. That guard therefore does not apply to the tree pane: a tree update is applied and emitted in every case, and a lower `tree.cursor` drops nothing. pro-d426's monotonic rule stays for `inbox` and `plan`. Order is the sources', not the cursor's: the stream's frames keep their pipe order (13.8), and the spawn path's updates come from the one coalesced loop of 6.3 rule 3.
- This is why air-n14 happens at all and why one seam fixes both bugs: `activeLeadPlanId` reads the tree, so a tree that stops advancing nulls the Plan pane's `visiblePlan` gate.

### 14.2 The Plan pane's empty state (air-d28)

- The Plan pane's empty state — `PlanView` with `plan === null`, `ManagerView` with `context === null`, and the Plan pane of 6.11's wiring with no Context for the selected session — renders the text `Select a lead to see plans`, and nothing else.
- `CpdFlowCanvas` renders **only in the Documentation dialog** (`web/src/ui/dialogs/DocsDialog.tsx`, the `flowchart` tab), which the status bar's `Documentation` item opens (`web/src/ui/cockpit/StatusBar.tsx`). It renders in no pane and as no empty state: not in the Plan pane, not in the Manager view.

## 15. One WebSocket transport, the heartbeat and the serve-stream watchdog (Plan `webui-ws`)

Normative. ADR 0015 option B (air-d46) decides the three points below; docs/contract.md §5.5 "The WebUI transport and the serve-stream watchdog" and web/ELECTRON.md "WebSocket transport" carry the same statements. Where a statement of §4, 6.3, 6.6 or 13 is marked "Amended by 15.x", section 15 wins and the older statement is void.

### 15.1 The endpoint, the Origin check and the client transport (air-d46)

- **One endpoint.** One WebSocket endpoint, path `/cpd-ws`, on the pinned `ws` package (<https://github.com/websockets/ws>), wrapping the one `newConnection(send)` factory (6.6): one hub per accepted socket, one JSON text frame per wire message both ways (§4). The Electron main process binds it on `127.0.0.1` on a port the OS chooses (bind port 0); the Vite dev server hosts the same endpoint same-origin on its httpServer (6.6, amended by 15.1), replacing the `cpd:c2s`/`cpd:s2c` hot route. No test opens a real port (A6): the server socket is injected like the durable client's `SocketFactory` (6.4).
- **How the renderer learns the URL.** The desktop renderer reads the resulting `ws://127.0.0.1:<port>/cpd-ws` from the preload `contextBridge` — one read-only member of `window.cpdElectron` (`webSocketUrl`), handed over once at mount. No IPC channel carries WebUI data frames; `web/src/electron/transport.ts` (`createElectronTransport`) and the per-sender IPC hub wiring are deleted, and so is the dev-only hot transport of `web/src/main.tsx`. The browser build connects same-origin `/cpd-ws` on the Vite dev server. A remote thin client reaches the endpoint through `ssh -L` (ADR 0015 §Comparison, row R3).
- **The Origin check.** No token. At connect, before any hub is created, the endpoint accepts a connection whose `Origin` header is absent, `null`, or exactly `file://` — the origin the production Electron renderer sends, because the desktop main process loads it with `loadFile` (a `file://` page serializes `Origin: file://` on the WebSocket handshake) — or whose host is a loopback (`localhost`, `127.0.0.1`, `[::1]`); every other Origin is rejected and no hub is created for it. Accepting `file://` is as local as accepting `null`: only a page loaded from this machine's disk can send it, so no foreign `http(s)` origin slips in.
- **The client transport.** `createWebSocketTransport` implements the unchanged `Transport` interface (6.1): `connect()` begins one attempt, `open` arrives as `onStatus("open")`, `send` throws `WebError("internal")` when not open, `close` is final. A socket `close` or `error` — including a heartbeat failure (15.2) — is a real `closed` status, so the renderer's existing backoff and re-subscribe run (6.2): every reconnect resyncs by full `cockpit.snapshot`, no delta protocol and no `seq` resume. The transport carries the client side of the heartbeat and no reconnect logic.

### 15.2 The heartbeat and the stall watchdog (air-d46)

- **The server's protocol ping.** The server protocol-pings every socket every `WS_PING_MS` (RFC 6455 §5.5.2/§5.5.3) and terminates a socket that has not ponged within `WS_PONG_TIMEOUT_MS` of that ping. The terminate surfaces as `close` → `onStatus("closed")`, so a stalled transport is seen loud, never silent. A stalled transport means the socket; the pipe is watched by the watchdog below.
- **The client's wire ping.** The client sends a wire `ping` (4.1, amended by 15) every `WS_CLIENT_PING_MS` and requires the server's wire `pong` (4.2, amended by 15) within `WS_CLIENT_PONG_TIMEOUT_MS`; a `pong` that misses the deadline closes the socket from the client side, so a hung server is seen too. The client pings on the wire because a browser `WebSocket` cannot send protocol ping frames. The hub answers each `ping` with a `pong` at once (6.6, amended by 15.1), with no cockpit or session involvement; `pong` carries no state and the reducer changes nothing on it.
- **The stall watchdog** (in `web/src/server/cockpit/`, on the stream boundary 13.1 owns). A serve child that stays alive but stops emitting (the air-n13/n14/n15 freeze) is detected here, and the watchdog runs **whatever `ready()` says** (13.2): while the stream is ready, the safety tick skips the pane refresh (6.3 rule 6, 13.6) and the `events follow` fallback early-returns (13.6) — that gate is exactly what froze the panes. Because the stream is silent while the cockpit is idle, a bare idle timer would fire on healthy children, so the watchdog elicits frames: every `STREAM_PROBE_MS` while the stream is connected it writes a `refresh` request on the serve child's stdin (13.1; the child answers it at once, 13.8/§11a) and arms `STREAM_PROBE_TIMEOUT_MS`; any frame of any `t` inside the window answers the probe. A stall is declared when either signal fires: the **frame deadline** — the window closes with no frame at all; or the **`seq` non-advance** — the answering `snapshot`'s `seq` is not greater than the last `seq` seen from this serve process (every snapshot advances `seq`, §11a). A `hello` of the same child — the in-place re-exec of 13.9 — starts a new generation and resets the watchdog's `seq` baseline, so snapshot 1 of the re-exec is fresh, never read as a non-advance.
- **The stall's reaction.** On a stall the stream reports it loud once — one error on the stream's error path (13.8/13.9's error update), so the renderer gets `cockpit.error` and the cockpit error toast shows it — and **requests a resync**: the service reloads the panes by spawn (the fallback refresh of 6.3 rule 3), whatever `ready()` said. The serve child is then **restarted if it is still silent**: if the child still emits nothing, it is killed and relaunched with the restart backoff of 13.7, whose `hello` re-arms the watchdog. Probing stops while the stall is handled and resumes with the next child.
- **The desktop's health report (Plan `webui-empty`).** The desktop renderer reports `health:ok` to the main process — the report the main process records as the preview's health — **only after the first `cockpit.snapshot` arrives on the WebSocket transport**. A painted shell is not health: the earlier mount-time report (the shell rendered non-empty with no errors and the startup IPC resolved) said nothing about data, so a boot whose stream was dead still recorded healthy (air-n52: a healthy preview-health.json beside zero WebSocket frames). With the gate, a renderer that never receives a snapshot never reports, the boot stays `loading`, and the boot watchdog's loading grace flags it — the same "seen loud, never silent" rule the watchdogs above apply to the socket and the serve stream.

| Constant | Value | Meaning |
|---|---|---|
| `WS_PING_MS` | 30 s | the interval of the server's WS heartbeat protocol ping |
| `WS_PONG_TIMEOUT_MS` | 10 s | how long after a ping the server waits for the pong before it terminates the socket |
| `WS_CLIENT_PING_MS` | 30 s | the interval of the client's wire `ping` |
| `WS_CLIENT_PONG_TIMEOUT_MS` | 10 s | how long after a wire `ping` the client waits for the `pong` before it closes the socket |
| `STREAM_PROBE_MS` | 30 s | the interval of the watchdog's `refresh` probe on the serve stream; the safety tick's cadence (6.3) |
| `STREAM_PROBE_TIMEOUT_MS` | 10 s | how long after a probe the watchdog waits for any frame before it reports the stall |

### 15.3 The lower-cursor toast (air-d44)

- In the cockpit service's `apply` (6.3 rule 11, amended by 15.3), a pane update the guard would drop — a lower cursor on `inbox` or `plan`, the monotonic guard pro-d426 kept as 14.1 leaves it — is **not dropped silently**: the drop emits one `{kind:"error"}` update, so the store's `cockpit.error` is set and the cockpit error toast shows it (the toast surface of 6.11/DESIGN-panes 4.8; the report target is `routeErrorTarget`'s, 7). The pane keeps its held rows, and the next snapshot that carries an advancing cursor reconciles it.
- **The pane refresh contract is unchanged.** The cursor guard (6.3 rule 11 as 14.1 amends it) and the safety refresh (6.3 rule 6) keep the shape Plan `toast-fix`'s `webui-tree` change gives them, which lands separately: this section adds the toast on the drop, nothing else — no new pane source, no new refresh path, no tree-guard change.

### 15.4 Tests and limits

- Tests run without a real process, socket, timer or `cpd` (A6): the server socket and the client `WebSocket` are injected fakes driven by `emit*` helpers, the heartbeat and the watchdog run on the injected `Scheduler`, and no test opens a real port.
- Limits, stated plainly: the browser's automatic pong answering is the platform's, not the code's (unverified in a real browser); the constants above are the code's named constants, not configuration.

## 16. Amendments air-d44, air-d43: the error toast standard and the image-only message (Plan `toast-fix`)

Normative. Plan `toast-fix` decides the two points below after the WebUI bugs air-d44 (errors rendered inline in the chat) and air-d43 (an image-only chat message was refused). Where a statement of 3.9, 4.3, 6.2, 6.5, 6.8 or 11.3 is marked "Amended by 16.x", section 16 wins and the older statement is void.

### 16.1 Every error of the User's own actions is shown through the cockpit error toast (air-d44, amended by air-d77)

- In the WebUI every error of the User's own actions is shown through the cockpit error toast (the error Alert of 11.7, with its Report button). An error rendered inline in the chat breaks the standard. A failed submission leaves no row and no inline text in the transcript: its error appears once in the cockpit error toast, and the User retypes the message.
- **Amended by air-d77 (Plan `tool-style`): toasts are for the User's own actions only.** The cockpit error toast is raised only for an error of the User's own actions — a failed submit, a refused save, a refused merge or dismissal and the like. No agent tool failure raises a toast, at any point: not when the failed result first arrives, and not when a tail window carrying old failures is replayed as the transcript attaches or reconnects — a reloaded transcript raises no toast for the failures it replays.
- A failed agent tool call shows as a failed tool row: the row keeps its failed tone, its failed count and its place in the Chain of Thought, and the failure's record is the row itself (6.8's status text, amended above) — never the cockpit error toast.

### 16.2 An image-only chat message is valid and delivered (air-d43)

- A chat message that carries one or more images and no text is valid and is delivered: it means the User asks the agent to look at the image and find the problem. `invalid_submission` stays only for a submit with no text and no images; `PI_SEND_TEXT_MAX` still applies to the text that is sent (11.3).
- The Composer and the pi backend take it as any other submission: the images are written to the attachments directory as 11.3 says, the text sent is the `[Attached image: <path>]` lines alone, with no leading blank line (11.3, amended above), and the submission is echoed, queued and delivered like any other.

## 18. The web-to-CLI argv contract (Plan `desktop-e2e`; air-d54, air-d57)

Normative for every web module that builds a `cpd` command line. The full statement — the rule, the `cpd argv-check` command interface and the Gate test — is **docs/contract.md §5.6 "The web-to-CLI argv contract"**; where the two differ, docs/contract.md wins (this file's preamble rule, section 10 excepted).

- **One shared argv module.** No web code spawns `cpd` with hand-built argv: every command line is built by `web/src/server/cpd-argv.ts`, which exposes the discriminated union `CpdCommand` (one variant per cpd command the web sends) and `toArgv(cmd: CpdCommand): string[]` — the argv after the program name. Every spawn flavour (the `cpdExec.run`, `follow.start` and `stream` boundaries of `web/src/server/plugin.ts`, and the Electron paths `web/src/electron/update/`, `pty.ts`, `pty-tabs.ts`) consumes only `toArgv` output; the call-site modules (`web/src/server/cockpit/`, `web/src/server/pi/backend.ts`, `web/src/server/durable/backend.ts`, and the Electron paths) construct `CpdCommand` values, never their own arrays.
- **The Worker count's argv is in the contract (Plan `settings-workers`, air-d69).** The Agent Tiers dialog's save (17, 19) sends `cpd config concurrency set <n>` (commands.md §14.2c) as the `CpdCommand` variant `{ readonly kind: "config.concurrency.set"; readonly n: number }`, whose `toArgv` returns `["config", "concurrency", "set", String(n)]`. The builder's enumeration list `allCpdCommands()` carries the variant, so the Gate test below covers the argv with no change to the test: it enumerates every variant, and an argv with no variant has no test.
- **The Gate test.** `web/src/server/cockpit/argv-contract.test.ts` builds every `CpdCommand` variant from `cpd-argv.ts` alone, spawns one `cpd argv-check` process, feeds the argv arrays on stdin, and fails on any parse error (docs/contract.md §5.6 states the command's stdin/stdout/exit interface exactly). It runs in the Gate (`pnpm run verify`, step `verify:web`), so a CLI grammar change that breaks a web argv fails the Gate before merge.

## 17. The Agent Tiers Save sends all four role Tiers and toasts a refusal (Plan `toast-fix`)

Normative. Plan `toast-fix` decides the two points below for the Agent Tiers dialog (`web/src/ui/dialogs/SettingsDialog.tsx`, the Settings of `web/src/ui/DESIGN.md`) and its Save: a Save of one or two roles was refused by `cpd config tiers set` with `the following required arguments were not provided:`, and the dialog rendered that refusal as an inline red line (`data-part="settings-error"`) instead of the cockpit error toast of 16.1.

### 17.1 The Save sends all four role Tiers

- The Save runs `cpd config tiers set` with **all four** role Tiers — `--auxiliar`, `--worker`, `--lead` and `--manager`, the values the dialog shows, whether or not the User changed them — because `cpd config tiers set` accepts a save only with all four role Tiers (its `--help` usage line is `cpd config tiers set --auxiliar <T> --worker <T> --lead <T> --manager <T>`).
- The rest of the save keeps its TUI order: the experimental switches are set one by one, then `cpd session retier --all` runs when a Tier value differs from the read; a refusal stops the save at the first failing step. **Amended by 19.2 (Plan `settings-workers`):** when the Worker count changed, `cpd config concurrency set <n>` runs right after the Tiers write, before the switches.

### 17.2 A Save refusal is shown through the cockpit error toast

- A Save refusal — a `settings.saved` with `ok: false` (§4) — is shown **once in the cockpit error toast** (the error Alert of 11.7, the toast surface of 16.1), with the whole error message: the CLI's refusal text, in full, uncut.
- The dialog renders no inline error and no inline red line: the `data-part="settings-error"` line of `SettingsDialog.tsx` is gone. A failed Save leaves the dialog open with the values the User typed, and the error lives in the toast only.


## 19. The Agent Tiers dialog shows and saves the Worker concurrency (Plan `settings-workers`; air-d69)

Normative. Plan `settings-workers` decides the points below for the Agent Tiers dialog (`web/src/ui/dialogs/SettingsDialog.tsx`, the WebUI half of the settings surface of air-d69; the TUI half is commands.md §15 and docs/contract.md's TUI settings section). `web/src/server/cockpit/settings.ts` is the server module that reads and saves; `web/src/protocol/cockpit.ts` holds `SettingsModel` and `SettingsChanges` (§4); `cpd config concurrency set <n>` (commands.md §14.2c) is the one write path, shared with the TUI (§17.1's argv is untouched).

### 19.1 The settings payload carries `concurrency`

- `SettingsModel` gains `concurrency: number`: the machine-wide Worker count (config `concurrency`, docs/contract.md §8.2, commands.md §14.2c). It comes from the **same read as the Tiers**: the top-level `concurrency` key of the `cpd config` JSON (docs/commands.md §14.2's example carries it), so there is **no second read argv** and no new `CpdCommand` variant for the read — `readSettings` (settings.ts) reads it from the reply of `toArgv({kind:"config.read"})` it already holds. A reply whose `concurrency` is not an integer in 1 to 16 inclusive is refused `cpd_malformed_json` naming the field `concurrency` (the same `bad(field)` refusal the Tier keys get, §6.3's error mapping).
- `SettingsChanges` gains `concurrency?: number`: present only when the User changed the value. The wire boundary parses it once (§4.3): an integer 1 to 16 inclusive, else `invalid_message` with `details.field` `concurrency` — no clamp, no silent default (air-d70).
- **Where the field is shown:** in the Agent Tiers dialog, inside the **"Models + Tiers" section, after the four role-Tier inputs** (the config's `concurrency` is the machine-wide Attempt limit, the count the StatusBar's workers pill shows after the `/` of its `workers Q·R·T/M` text, `data-part="workers"`). The field is a numeric input with the accessible label `concurrency` (`aria-label="concurrency"`, the role inputs' `aria-label="<role> tier"` pattern), prefilled with the read's value. A value outside 1 to 16 marks the input invalid (`aria-invalid`, the same mechanism the Tier inputs use) and disables Save; the value is never clamped (air-d70).

### 19.2 The Save writes all four role Tiers and then the count

- The Save keeps §17.1's order and adds the count right after the Tiers: `cpd config tiers set` with all four role Tiers (when any Tier changed), then **`cpd config concurrency set <n>` (when the count changed)**, then the experimental switches one by one, then `cpd session retier --all` when a Tier changed. A count that equals the read's value writes nothing.
- The count's argv goes through the **shared argv builder** (18): the variant `{ readonly kind: "config.concurrency.set"; readonly n: number }` of the `CpdCommand` union, whose `toArgv` returns `["config", "concurrency", "set", String(n)]`. No call site builds the array itself; `saveSettings` constructs the `CpdCommand` value and calls `toArgv`.

### 19.3 Refusals: the range, the toast, and what a lowered count stops

- **The range and its refusal (air-d70).** The accepted range is **1 to 16 inclusive**. A value outside it is refused loudly — `config_invalid` (`details.key` `concurrency`), no clamp, no silent default — both by the command (commands.md §14.2c) and, a hand-edited `config.toml` no exception, when the file is loaded (docs/contract.md §8.2). The refusal reaches the web as any CLI refusal does (`cpd_failed` with the CLI's message, §6.3's mapping) and is shown like any Save refusal (17.2).
- **A refused save stops the remaining writes and is shown as the error toast (air-d44), never inline.** The first failing write stops the save: the writes after it do not run (the same first-refusal rule §17.1 states for the switches), the dialog stays open with the values the User typed, and the whole CLI refusal text appears **once in the cockpit error toast** (the toast surface of 16.1) — never as an inline line in the dialog (the `data-part="settings-error"` paragraph of 17.2 stays gone). A successful save still closes the dialog.
- **Lowering the count stops nothing (air-d71).** A save that lowers `concurrency` below the number of running Workers stops nothing: the engine reads the count from the config file on each tick, so the set takes effect without a restart, and it gates **new starts only** — the running tenants keep their slots to the end and the urgent one-beyond-`concurrency` rule is unchanged (docs/contract.md §3.7 E3, §8.2). Nothing in the web waits on or re-reads the count after the save; the workers pill (its `/M`, `data-part="workers"`) picks the new value up on the engine's next tick.

## 20. The cockpit status pane on the serve stream (Plan `status-workers`)

Normative. Plan `status-workers` decides the points below: the Workers counts and the incident state the StatusBar shows are carried by `cpd cockpit serve` itself, as one more pane of the snapshot it already writes (the frame grammar of commands.md §11a), so the WebUI reads the engine's own numbers — one source of truth (the User's request of 2026-11-07 22:46 EST; TUI parity, air-d98) with no second computation and no second transport. No new frame kind: the stream still speaks only `hello`, `snapshot`, `event`, `plan_show` and `error` (the `Frame` enum of core/src/cockpit/mod.rs).

Grounding: `$CPD_HOME/run/status-workers/explore/scout-ws-workers.md`, `scout-webui-status.md`, `scout-tui-status.md` (the explore artefacts of this Plan, cited as `scout-<x> §n`).

### 20.1 The serve snapshot's fifth pane: `status`

- **The pane's exact name is `status`.** The `snapshot` frame's `panes` object gains the key `status` beside the four it has: `{"t":"snapshot","seq":N,"panes":{"context":V,"plan":V,"session":V,"inbox":V,"status":S}}`. The four existing panes and their values are unchanged; each snapshot stays full (no diffs), the status pane included.
- **`S` is read fresh from the store on every snapshot**, through the one open Env `serve` already holds (commands.md §11a "Lifecycle"; no second store connection), and it carries `ok: true` like every pane value, so the parse rule of 13.8 ("a pane whose `ok` is not `true` rejects the whole snapshot") applies to it unchanged. Its fields are the engine's own numbers, exactly as the `@cpd_workers` step computes them (engine/src/watch/workers.rs:34-53, 86-93), all four counts read in one read transaction (workers.rs:56-66):

```json
S = {
  "ok": true,
  "workers": {
    "queued":  <u32>, // every Job with status 'queued', the waiting ones included (QUEUED, engine/src/watch/workers.rs:37)
    "waiting": <u32>, // the Ready Jobs, E3's rule: the one shared READY query (engine/src/watch/schedule.rs:101, read at workers.rs:62)
    "running": <u32>, // the tenant Jobs of pool 'workers', the starter's occupancy, a failed Job included: the one shared tenancy query (core/src/resource/mod.rs:38, HELD at engine/src/watch/workers.rs:34)
    "limit":   <u32>  // config `concurrency`, read fresh on every snapshot (engine/src/watch/workers.rs:86-93)
  },
  "incident": {
    "on":         <boolean>,      // incident mode is on: the `incident` row exists (cpd_core::incident::is_on, core/src/incident.rs:121)
    "started_at": <string|null>,  // RFC 3339, the `incident` row's `started_at` (engine/src/watch/workers.rs:146-148); null when `on` is false
    "lead":       <string|null>   // the incident Lead's Session id (incident::lead, core/src/incident.rs:151-179); null when `on` is false or no Lead resolves
  }
}
```

- The invariant of `incident`: `on` is true exactly when the `incident` row exists; when `on` is false, `started_at` and `lead` are null. There is no state where the counts or the incident are half-carried: the pane is written only from a read that succeeded — a store error while reading it ends `serve` as commands.md §11a says any store error does — and no stale or partial pane value is ever written.
- **The four counts are the TUI's `workers Q·W·N/M` numbers in its display order** (engine/src/watch/workers.rs:73-80): `queued` = Q, `waiting` = W (the count the WebUI's `StatusModel.workers` names `ready`, mapped by 6.7's `barStatus`), `running` = N (the count `StatusModel.workers` names `tenants`), `limit` = M (`concurrency`). The pane keeps the engine's field names; the renderer's mapping to the pill's labels is unchanged.
- **Protocol-version rule: adding the pane does not raise the serve stream's `protocol`** (the `1` of `hello`, core/src/cockpit/mod.rs:68). The change is additive in both directions, so no reader is broken: a web built before the pane checks the four pane keys it knows and ignores the unknown one (`parseFrame` reads `panes["context"|"plan"|"session"|"inbox"]` and passes the rest through, web/src/server/cockpit/stream.ts:57-62), and a web built after it reads a snapshot without `status` as "this stream carries no status pane" and keeps the spawn/extras status read it already has (6.3, `status.ts`) — degraded freshness, never a misread. The rule for the future: the serve stream's `protocol` is raised only when a change would make an older web misread a frame it already accepts; adding a pane key is not such a change.

### 20.2 How the pane is delivered

- **On the serve stream, the pane travels on the `snapshot` only.** No event kind carries the counts (the `@cpd_workers` step writes no Event, engine/src/watch/workers.rs:10) and the stream has no patch frame to carry a pane delta, so none is invented. This is enough: every batch of committed changes ends in a snapshot (commands.md §11a "Batches and latency"), and the watchdog's `refresh` request forces one on demand (15.2), so any committed change that moves a count or the incident reaches the WebUI backend with the next snapshot — even when the batch's Events name no pane the service refreshes.
- **On the WebUI's one WebSocket (§15), it is delivered both ways the wire already has — no new message type:** the cockpit service maps the serve snapshot's `status` pane onto the optional `status` field of `cockpit.snapshot` (§4.2, amended), and emits `cockpit.patch` with `pane: "status"` when a fresh snapshot's pane differs from the one held. Both shapes are the ones the store's reducer already files (`state/store.ts`, the `cockpit.snapshot` merge and the computed-key `cockpit.patch` branch); `CockpitPane` (§4.2) gains `"status"`, which the server already sends and the store already accepts.
- **One source of truth.** With the pane, `StatusModel`'s `workers` and `incident` fields are fed from the serve stream, and the `@cpd_workers` tmux scrape of `web/src/server/cockpit/status.ts` (the regex over the TUI's display string, `scout-ws-workers §(d)`) is retired for those fields: the WebUI reads the same store numbers the TUI's step reads, with no second computation over a display format. The incident pill's elapsed text stays the renderer's own rendering (6.7), computed from the pane's `started_at`.
- **Tests and limits:** the pane's parse and delivery are tested with fakes like every stream path (13.10, A6 — no real process, socket or `cpd`); the pane's live values are unverified until the code Jobs of the Plan run them (unverified).

## 21. The StatusBar's Workers segment and its clicks (Plan `status-workers`)

Normative. Plan `status-workers` decides the points below for the WebUI's status bar (the User's request of 2026-11-07 22:46 EST; TUI parity, air-d98): the Workers segment and the incident pill draw the TUI's exact texts from the serve status pane (§20), and both segments answer clicks the TUI's pills do not. The TUI's own contract and behaviour are unchanged (docs/contract.md §6.1).

Grounding: `$CPD_HOME/run/status-workers/explore/scout-tui-status.md`, `scout-webui-status.md`, `scout-ws-workers.md`, `scout-settings-dialog.md`, `scout-webui-lead-open.md` (cited as `scout-<x> §n`).

- **The segment's exact text is the TUI's long form** (engine/src/watch/workers.rs:68-80): `workers Q<U+F0798>W<U+F0798>N/M` — the literal label `workers `, then Q = the pane's `workers.queued`, the separator U+F0798 (the TUI's Nerd Font glyph), W = `workers.waiting` (the Ready Jobs), U+F0798, N = `workers.running` (the tenant Jobs), then `/` and M = `workers.limit` (config `concurrency`). No pluralisation, no padding, no per-state colour: the pill keeps today's `text-fg-dim` look. This supersedes the `·` separator of 6.7 and DESIGN 3b: the web draws the TUI's text, not a variant of it.
- **The short form is out of scope.** The TUI's `Q<U+F0798>W<U+F0798>N/M` (engine/src/watch/workers.rs:73-80) is its narrow-client fallback, chosen per client width by tmux (core/src/host/tmux.rs:305). The web makes no such choice: it always draws the long form (the bar clips with `overflow-hidden`, DESIGN 3b), and it never reads the `@cpd_workers_short` option.
- **Data source: the serve status pane (§20), live from the engine.** The segment and the incident pill read the `status` pane of §20, delivered on `cockpit.snapshot.status` and `cockpit.patch {pane:"status"}`. With the pane:
  - the WebUI no longer reads the tmux option `@cpd_workers` — the regex over the TUI display string in `web/src/server/cockpit/status.ts` is removed, not kept as a fallback;
  - the WebUI no longer runs `cpd incident.status`;
  - the 30 s poll of the status read (the extras `refreshExtras` on `SAFETY_REFRESH_MS = 30_000`, 6.3) is removed for these fields: the counts and the incident reach the bar when the engine commits them, with the next serve snapshot (§20.2's freshness rule), not on a timer.
- **The incident pill carries the Lead.** `StatusModel.incident` gains the pane's `incident.lead`: `{ startedAt: string, lead: string | null } | null` (§4.2, amended; `web/src/protocol/cockpit.ts`). The pill's text is unchanged: `Incident Mode: <elapsed>`, the whole-minute elapsed of 6.7 and DESIGN 3b, rendered by the renderer from `started_at`.
- **Click on the Workers segment opens the Settings dialog.** The TUI's Workers pill is non-interactive (scout-tui-status §e), so this is a deliberate WebUI deviation (the User's request). The click runs the App `onSettings` handler exactly as the Settings button does (6.11, DESIGN 3b): `connection.readSettings()`, then the Agent Tiers dialog (`web/src/ui/dialogs/SettingsDialog.tsx`).
- **Click on the incident pill opens the incident Plan's Lead.** When `incident.lead` is a session id, a click calls `connection.selectSession(lead)` (6.11's wiring; `web/src/state/connection.ts:21`), attaching the Lead in the chat slot — the same effect as opening it from the tree. With `lead` null (no Lead resolves, §20.1) the pill draws but a click does nothing: never a fallback to another session.
- **Not built:** the TUI's short form (above), and no click on any other pill: runner, preview, drift and version stay non-interactive (DESIGN 3b, Not wired).
