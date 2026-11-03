# web/ Phase 1 contract

The complete contract of the clean-slate `web/` package, Plan `webui-phase1-work` (Phase 1, "make it work", of cpd-webui). Workers write tests from this file alone. Where two statements conflict, the later section of this file does not win silently: ask the Lead (`cpd ask`).

Citations: `scout-cockpit.md`, `scout-durable.md`, `scout-pi.md` are the explore artefacts of this Plan (`$CPD_HOME/run/webui-phase1-work/explore/`), cited as `scout-<x> §n`. Repo paths are relative to the repo root. `node_modules/@earendil-works/pi-durable/dist/...` and `.../pi-ai/dist/types.d.ts` are the installed typings (pi-durable 1.3.0, read this run). Every unconfirmed fact is tagged `(unverified, O<n>)` and listed once in section 9 "Open items". A value this file does not cite is a decision of this contract, called a **Phase 1 choice**.

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
| pro-d17 | `web/package.json` is byte-identical, `pnpm-lock.yaml` is not edited: no dependency is added or removed. The transport is Vite `server.ws` custom events, dev bench only. |

### 1.3 Accepted assumptions

| Id | Assumption |
|---|---|
| A1 | Browser to server transport is Vite `server.ws` custom events `cpd:c2s` (client to server) and `cpd:s2c` (server to client), through `import.meta.hot.send/on` in the browser. Dev bench only; no auth beyond Vite's localhost bind. |
| A2 | Only existing dependencies: react 19, react-dom, react-markdown, remark-gfm, vite, @vitejs/plugin-react, vitest, jsdom, typescript. No Tailwind, shadcn, radix, router. Unstyled semantic HTML plus one small plain CSS file (`web/src/ui/app.css`). UI tests use `react-dom/client` + `act` from `react` under `// @vitest-environment jsdom`, no testing-library. |
| A3 | The durable backend implements pi-durable **1.3.0** host frames (`snapshot`/`events`) only. The 1.5.0 `ConversationView` shape cannot be checked (no `cpd:pi-1-5-upgrade` branch or Plan exists; scout-durable §6). The code keeps ONE dialect seam (`web/src/server/durable/frames.ts`) so a 1.5.0 adapter is added later by Plan `durable-host-webui`. An unknown host version or frame shape fails loud with a typed `unsupported_host_version` / `protocol_error`, never a silent fallback. |
| A4 | pi-runner sessions: transcript read from the session JSONL (`file` of `cpd session list`), live by file watch, prompts through `cpd session send`, Tier through `cpd session retier`. There is NO `cpd session abort` and no way to answer a TUI dialog from outside (scout-cockpit §3, scout-pi §6), so for pi sessions abort, steer, images and interactive dialogs are declared unsupported in typed `capabilities` with a human reason; dialogs are shown read-only, reconstructed from the transcript; the UI disables those controls and shows the reason. |
| A5 | Window size is **40** transcript items for the tail and for each `loadEarlier` page (`WINDOW_SIZE`). |
| A6 | Every timer or clock is an injected `Scheduler`. No test uses real time, a real socket, a real file watcher, a real `cpd`/`git` process or the real network: all externals are injected boundaries, faked in the test. No test leaks a file, process, socket, handle or timer. |
| A7 | Only paths under `web/` are touched. `pi/durable-host/**`, `pi/durable-worker/**`, the root `package.json`, `pnpm-lock.yaml` and `pnpm-workspace.yaml` are never touched. |

### 1.4 Limitations (stated plainly)

- **A3, durable is 1.3.0 only.** A host whose `welcome.pi_durable` is not exactly `"1.3.0"`, or a `snapshot`/`events` frame that does not match the 1.3.0 shapes of section 6.4, is refused with `unsupported_host_version` (version) or `protocol_error` (shape). The session shows that error; nothing is guessed. Supporting pi-durable 1.5.0 is the later Plan `durable-host-webui`.
- **A4, pi sessions cannot be driven fully.** For a pi session: no abort (no `cpd session abort` exists), no steer, no image attachments, no answering of dialogs, no model/thinking selection (the Tier decides, see 6.5), no compaction. Each is `{supported:false, reason}` in `capabilities`. Dialogs are read-only reconstructions. A queued-submission list does not exist (`queued` is always `[]`).
- Footer: pi sessions report cost and context % as `unknown` with a reason (a full-file scan is forbidden by 6.5; the window size of the model is not known). Durable cost covers the active transcript only. No value is faked.
- Phase 1 does not render `cpd` session messages (`customType` `cpd`, `cpd-error`) or other custom message types: they are dropped. Only the four fold kinds of 3.2 render.
- One attached session per browser connection. No auth. Dev bench only (`vite serve`), never `vite build` output.
- No visual design (pro-d16).

## 2. Module map (the fixed file layout; no other source path is written)

| Module | Implementation | Tests |
|---|---|---|
| protocol | `web/src/protocol/domain.ts`, `wire.ts`, `cockpit.ts`, `runtime.ts` | covered by the tests of the modules that parse (`state/connection.test.ts`, `server/hub/connection.test.ts`, `server/cockpit/*.test.ts`) |
| state | `web/src/state/transcript.ts`, `store.ts`, `connection.ts` | `transcript.test.ts`, `store.test.ts`, `connection.test.ts` |
| cockpit-srv | `web/src/server/cockpit/cli.ts`, `readmodel.ts`, `events.ts`, `actions.ts` | `readmodel.test.ts`, `events.test.ts`, `actions.test.ts` |
| durable-srv | `web/src/server/durable/client.ts`, `frames.ts`, `backend.ts` | `client.test.ts`, `frames.test.ts`, `backend.test.ts` |
| pi-srv | `web/src/server/pi/jsonl.ts`, `window.ts`, `watch.ts`, `backend.ts` | `jsonl.test.ts`, `window.test.ts`, `watch.test.ts`, `backend.test.ts` |
| hub-srv | `web/src/server/hub/connection.ts`, `registry.ts`, `web/src/server/plugin.ts` | `hub/connection.test.ts`, `hub/registry.test.ts`, `plugin.test.ts` |
| ui-cockpit | `web/src/ui/cockpit/ContextTree.tsx`, `PlanView.tsx`, `Inbox.tsx`, `Shell.tsx` | `ContextTree.test.tsx`, `PlanView.test.tsx`, `Inbox.test.tsx`, `Shell.test.tsx` |
| ui-chat | `web/src/ui/chat/Transcript.tsx`, `ToolRow.tsx`, `Folds.tsx`, `ChatHeader.tsx`, `Footer.tsx` | `Transcript.test.tsx`, `ToolRow.test.tsx`, `Folds.test.tsx`, `ChatHeader.test.tsx`, `Footer.test.tsx` |
| ui-composer | `web/src/ui/composer/attachments.ts`, `Composer.tsx`, `TierSelector.tsx` | `attachments.test.ts`, `Composer.test.tsx`, `TierSelector.test.tsx` |
| ui-dialogs | `web/src/ui/dialogs/AskCard.tsx`, `PlanReviewCard.tsx` | `AskCard.test.tsx`, `PlanReviewCard.test.tsx` |
| app | `web/src/ui/App.tsx`, `ChatPane.tsx`, `web/src/main.tsx`, `web/src/ui/app.css` | `web/src/ui/App.test.tsx` |
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
  readonly blocks: readonly AssistantBlock[]; // order of the message `content`; toolCall blocks are NOT blocks (they are ToolItems)
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

export type FoldKind = "cpd-context" | "cpd-plan-decision" | "cpd-plan-summary" | "cpd-skills";
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
// Dropped (no item): customType `cpd-first-message`, `cpd-reminder`, any message with display === false, and every other customType (section 1.4).

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
// durable: `dur:<index>`   the index into the server-side normalised transcript of the earliest loaded item

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
  readonly cost: Stat<number>; // USD, sum of assistant usage.cost.total
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
  readonly interactive: boolean; // false = read-only reconstruction (A4); the UI renders no controls
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
| `editor` after a `Change` answer | consumed (above) |
| any other `method`/`purpose` (`confirm`, `input`, other `select`) | `protocol_error` `unsupported dialog <method>/<purpose>`; the backend answers it with `ui_response {cancelled: true}` so the host is not stuck |

### 3.9 Attachments and errors

```ts
export type Attachment =
  | { readonly kind: "image"; readonly name: string; readonly mimeType: ImageMime; readonly dataBase64: string }
  | { readonly kind: "text"; readonly name: string; readonly text: string };
export interface WireImage { readonly mimeType: ImageMime; readonly dataBase64: string } // carried on `session.submit`; text attachments are inlined into `text` before sending (6.9)

export type ErrorCode =
  | "protocol_error" // a frame/line/JSON did not match its schema (host frame, wire message from the server)
  | "unsupported_host_version" // durable host welcome.pi_durable !== "1.3.0" (A3) or protocol not negotiable
  | "host_unreachable" // the durable socket cannot be opened (missing, refused, path too long)
  | "host_disconnected" // the socket closed or `bye` arrived while attached
  | "host_error" // the host answered `error`; details {hostCode, ...} carries the host ErrorCode (scout-durable §3)
  | "session_not_found" // id not in the cockpit snapshot / not attached
  | "session_file_missing" // a pi session row without `file`
  | "session_file_unreadable" // stat/read of the pi file failed
  | "malformed_line" // a pi JSONL line is not valid JSON or has no `type`; details {line?, offset}
  | "cpd_failed" // a cpd command exited non-zero; details {cpdCode, exitCode, message}
  | "cpd_malformed_json" // cpd printed something that is not the expected JSON
  | "invalid_message" // a wire message failed validation
  | "unknown_action" // a cockpit action kind is not one of the listed kinds
  | "unsupported_capability" // the session's capabilities say no; message = the capability reason
  | "invalid_submission" // empty text, or text longer than the backend allows
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

## 4. `web/src/protocol/wire.ts`: the browser/server wire

Carried as Vite custom events (A1): the browser calls `import.meta.hot.send("cpd:c2s", message)` and listens with `import.meta.hot.on("cpd:s2c", handler)`; the server listens with `server.ws.on("cpd:c2s", (data, client) => ...)` and answers with `client.send("cpd:s2c", message)` (`vite/dist/node/index.d.ts:1833-1866`, `NormalizedHotChannelClient.send(event, payload)` `:1394-1403`, vite 8.3.5). Payloads are JSON-serialisable; every message is an object with a string `type`.

### 4.1 Client to server

```ts
import type { Attachment, Cursor, ConfigureChange, DialogResponse, WhenBusy, WireImage } from "./domain.ts";
import type { CockpitAction } from "./cockpit.ts";

export type ClientMessage =
  | { type: "cockpit.subscribe" } // idempotent; the server answers with cockpit.snapshot (all panes)
  | { type: "cockpit.selectPlan"; planId: string | null } // choose the Plan shown in the Plan pane; null clears it
  | { type: "cockpit.action"; actionId: string; action: CockpitAction } // actionId: client correlation id, non-empty
  | { type: "session.attach"; sessionId: string } // attaches this connection to that session; an already attached session is detached first
  | { type: "session.detach" } // idempotent
  | { type: "session.loadEarlier"; sessionId: string; before: Cursor }
  | { type: "session.submit"; sessionId: string; clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy }
  | { type: "session.abort"; sessionId: string }
  | { type: "session.abortSubmission"; sessionId: string; clientSubmissionId: string }
  | { type: "session.compact"; sessionId: string; instructions?: string }
  | { type: "session.configure"; sessionId: string; change: ConfigureChange }
  | { type: "session.listModels"; sessionId: string }
  | { type: "dialog.respond"; sessionId: string; dialogId: string; response: DialogResponse };
```

`session.submit.images` holds only images (`WireImage`); text-file attachments are already inlined into `text` (6.9). `attachments` of the Composer (`Attachment`) never cross the wire.

### 4.2 Server to client

```ts
import type { Capabilities, Cursor, Dialog, DialogOutcome, FooterStats, ModelChoice, QueuedSubmission, SessionRef, TranscriptItem, Window, WireError } from "./domain.ts";
import type { ContextTreeModel, PlanViewModel, InboxModel, ActionOutput } from "./cockpit.ts";

export type CockpitPane = "tree" | "plan" | "inbox";
export type CockpitPaneData =
  | { pane: "tree"; data: ContextTreeModel }
  | { pane: "plan"; data: PlanViewModel | null } // null = no Plan selected
  | { pane: "inbox"; data: InboxModel };

export interface SessionMeta { ref: SessionRef; footer: FooterStats }

export type ServerMessage =
  // cockpit
  | { type: "cockpit.snapshot"; tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel } // all three panes; sent on subscribe
  | ({ type: "cockpit.patch" } & CockpitPaneData) // replaces ONE pane (an invalidation refresh); "patch" = per-pane replacement
  | { type: "cockpit.error"; error: WireError } // a refresh failed; the previous snapshot stays valid
  | { type: "action.result"; actionId: string; ok: true; output: ActionOutput }
  | { type: "action.result"; actionId: string; ok: false; error: WireError }
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
  | { type: "session.error"; sessionId: string | null; clientSubmissionId?: string; error: WireError }; // sessionId null: not tied to a session (invalid_message, session_not_found of an attach)

export type SessionServerMessage = Extract<ServerMessage, { type: `session.${string}` }>; // what a SessionBackend emits
```

Message semantics (server behaviour):

- **Order on attach.** `session.attached`, then `session.window {mode:"tail"}`, then `session.footer`, `session.status`, then `session.dialog` for each open dialog, oldest first. Live messages follow. A message for a session other than the connection's attached one is never sent.
- `session.window mode "tail"` carries the last `WINDOW_SIZE` items (A5). `mode "earlier"` carries the page ending before the `before` cursor and a fresh `cursor`.
- `session.append` items never have ids already present; `session.update` items always have ids already sent. The reducers still merge by id in both cases (4.4).
- `session.delta` addresses `blocks[blockIndex]`: `blockIndex === blocks.length` creates a new block of `blockType`; a smaller index appends `text` to that block.
- `session.submitAck` answers each `session.submit` that the backend accepted: `queued: true` when the host queued it behind a running turn, else false. A rejected submit answers `session.error` with `clientSubmissionId` set.
- `session.status.busy` is true while a turn runs. `queued` lists accepted-but-not-started submissions (durable only; always `[]` for pi).
- A `session.dialog` with `interactive: false` is a read-only reconstruction.
- `action.result ok:true` carries `output` (section 5.4); `ok:false` carries the `cpd_failed` / `unknown_action` error.

### 4.3 Validation (parsed once at each edge)

```ts
export function parseClientMessage(raw: unknown): ClientMessage; // used ONLY by server/hub/connection.ts
export function parseServerMessage(raw: unknown): ServerMessage; // used ONLY by state/connection.ts
```

Both are pure and throw `WebError`:

- not an object, no string `type`, or an unknown `type` -> `invalid_message` (server messages: `protocol_error`);
- a missing field, a wrong primitive type, an empty `sessionId` / `clientSubmissionId` / `actionId` / `dialogId` -> same codes, `details.field` names the field;
- `session.submit`: `images.length <= IMAGE_MAX_COUNT`, every `mimeType` in `IMAGE_MIME_TYPES`, summed `dataBase64.length <= IMAGE_MAX_TOTAL_BASE64`, `whenBusy` in `followUp|steer`, `text` a string (emptiness is the backend's `invalid_submission`);
- `session.configure`: exactly one `change.kind` of `model|thinking|tier`;
- `dialog.respond`: `response.method`/`outcome` pair from `DialogResponse`; `changes` needs `feedback.trim() !== ""`;
- `cockpit.action`: the action goes through `parseCockpitAction` (section 5.5), whose failures are `unknown_action` (unknown kind) and `invalid_message` (bad field of a known kind);
- `Dialog` objects, `Window`s (`cursor === null` iff `!hasEarlier`) and `TranscriptItem`s are checked structurally (kind discriminator, required fields, FoldItem `foldKind` in the four kinds).

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
3. A Session node per row of `session list`: with `plan === null` it goes to `ContextNode.sessions`; otherwise to `PlanNode.sessions` of Plan `plan` in Context `context`, the Lead (`plan_lead === true`) first.
4. A Plan or Session whose Context (or Plan) is absent from the other command's reply (the four commands run at different instants) is attached to a synthesised node: a Context `{id, title: id, archived: false, records: 0, openQuestions: 0}` / a Plan with the row's own fields it lacks set to `phase: "unknown"`, zero counts, `leadSessionId: null`. This is explicit, never silent dropping.
5. `SessionRef.key` rule: `key` if a non-empty string, else `name` if a string, else `id`.

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
  | { kind: "plan.reject"; planId: string; review: string };
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

Pi prompt sending is not a cockpit action: it is `sessionSendArgs` (6.3, `actions.ts`).

### 5.5 Parser

```ts
export function parseCockpitAction(raw: unknown): CockpitAction;
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
export interface ResolvedSession { readonly ref: SessionRef; readonly file?: string; readonly socketPath?: string } // from the registry (6.6)
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

The real browser transport (`main.tsx`, NOT tested): `send` = `import.meta.hot.send("cpd:c2s", raw)`; `onMessage` = `import.meta.hot.on("cpd:s2c", ...)`; status from `import.meta.hot.on("vite:ws:connect" | "vite:ws:disconnect", ...)` (declared in `vite/types/customEvent.d.ts:16-17`; their runtime timing is unverified, O11). The real `Scheduler` is `{ now: Date.now, after: (ms, run) => setTimeout }`, constructed in `main.tsx` and `plugin.ts` only.

Test helpers (`web/src/testing/scheduler.ts`): `class FakeScheduler implements Scheduler { advance(ms: number): void /* runs due timers in time order, synchronously */; pending(): number; }` and `class FakeTransport implements Transport { sent: unknown[]; connects: number; emitMessage(raw): void; emitStatus(s): void; closed: boolean }`. `web/src/testing/render.tsx`: `render(element): { container: HTMLElement; rerender(el): void; unmount(): void }` using `createRoot` + `act`, and `click(el)`, `type(el, text)` helpers that dispatch real DOM events inside `act`.

### 6.2 State module `web/src/state/`

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
  readonly state: "sending" | "failed"; readonly error?: WireError;
}
export interface SessionView {
  readonly id: string;
  readonly attached: boolean; // false from select() until session.attached arrives
  readonly ref: SessionRef | null; readonly title: string | null; readonly footer: FooterStats | null; readonly capabilities: Capabilities | null;
  readonly transcript: TranscriptState;
  readonly busy: boolean; readonly queued: readonly QueuedSubmission[];
  readonly pending: readonly PendingSubmission[];
  readonly dialog: Dialog | null; // the active dialog
  readonly models: readonly ModelChoice[] | null;
  readonly error: WireError | null; // last session.error without a clientSubmissionId
}
export interface AppState {
  readonly connection: ConnectionStatus; readonly retryAttempt: number; // reconnect attempts since the last open
  readonly cockpit: { readonly tree: ContextTreeModel | null; readonly plan: PlanViewModel | null; readonly inbox: InboxModel | null; readonly selectedPlanId: string | null; readonly error: WireError | null };
  readonly selectedSessionId: string | null;
  readonly session: SessionView | null; // null iff selectedSessionId is null
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

- `cockpit.snapshot` sets the three panes and clears `cockpit.error`; `cockpit.patch` replaces exactly the named pane; `cockpit.error` sets `cockpit.error` and keeps the panes.
- `selectSession` with an id: `selectedSessionId = id`, `session = { id, attached:false, everything else empty/null, transcript: emptyTranscript }`; the same id again is a no-op; `null` clears both. (The detach/attach messages are sent by `connection.ts`, 6.2 below, not by the reducer.)
- A server `session.*` message whose `sessionId` is not `selectedSessionId` is ignored (a late message of a detached session never lands).
- `session.attached` sets `attached:true`, `capabilities`, `title`, `ref`, `footer` from `meta`. `session.window` mode `tail` -> `setTail`; mode `earlier` -> `prependEarlier`. `session.append` / `session.update` / `session.delta` -> the matching `transcript.ts` function (a thrown `WebError` is caught by `reduce`, stored in `lastError`, and leaves the state otherwise unchanged).
- `session.status` sets `busy` and `queued`. `session.footer` sets `footer`; `session.title` sets `title` (the header shows `title`; the footer and title also come from `session.attached.meta`).
- `session.submitAck` removes the pending submission with that `clientSubmissionId` (reconciled). `session.error` with `clientSubmissionId` marks that pending submission `state:"failed"` with the error; without it sets `session.error` (and, when `sessionId` is null, `lastError`).
- `submitPending` adds a `sending` submission; `dismissPending` removes one.
- `session.dialog` sets `dialog`; `session.dialogResolved` clears `dialog` only when its `dialogId` equals the active dialog's id; `session.models` sets `models`.
- `action.result ok:false` sets `lastError`; `ok:true` changes nothing (the panes refresh through events).
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
- `selectSession(id)`: dispatches `selectSession`; if a session was selected before, sends `session.detach` first; then, for a non-null id, `session.attach {sessionId}`. Selecting the same id again sends nothing.
- `loadEarlier()`: sends `session.loadEarlier {sessionId, before: transcript.cursor}` only when `hasEarlier` and a session is selected; otherwise no message.
- `submit(input)`: creates `clientSubmissionId` via `newId`, dispatches `submitPending` (`state:"sending"`, `imageCount = images.length`), sends `session.submit`, returns the id. Ack/failure arrive as server messages (reducer, above). Without a selected session: throws `WebError("not_attached")`.
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

export const COALESCE_MS = 250; // Phase 1 choice
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

1. `start()` runs `readModel.loadAll(null)`, stores the snapshot, then starts `follow.start(followArgs(cursor))` exactly once, where `cursor` = `tree.cursor`. A failure of `loadAll` rejects `start()` with its `WebError` and starts no follow. A second `start()` is a no-op.
2. Each follow line goes through `parseEventLine`; the service remembers `lastCursor = max(lastCursor, event.id)`; a malformed line emits `{kind:"error"}` and is skipped.
3. An event whose `panesFor` is non-empty adds those panes to a pending set; if no coalescing timer is pending one is scheduled `COALESCE_MS` later (`scheduler.after`). When it fires, `readModel.loadPanes(pending, selectedPlanId)` runs ONCE for the whole burst and one `{kind:"pane"}` update per pane is emitted. A burst of N events inside the window therefore triggers one refresh (never one CLI spawn per event or per render).
4. An event kind in no list triggers no `loadPanes`.
5. A failed refresh emits one `{kind:"error", error}` and keeps the previous snapshot; the pending set is cleared.
6. A safety refresh of the `plan` pane (when a Plan is selected) runs every `SAFETY_REFRESH_MS` through the Scheduler.
7. When the follow process exits while the service runs: restart `followArgs(lastCursor)` after `FOLLOW_RESTART_BASE_MS * 2^n` (n = consecutive restarts, reset by a received line, capped 30 000). Exit code 3 (`cursor_ahead`, `docs/commands.md` §11.2 precondition; live behaviour of `events follow` not run, O19) first reloads all panes (`loadAll`) and restarts from the new `tree.cursor`.
8. `stop()` kills the follow process, disposes every timer and subscription; nothing fires afterwards. `selectPlan` stores the id and loads only the plan pane.
9. `runAction(action)` delegates to `runAction(cpd, action)` of `actions.ts`; errors propagate as `WebError`.

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

Protocol facts: scout-durable §1-§8. Transcript dialect (A3): pi-durable **1.3.0**, shapes read from `node_modules/@earendil-works/pi-durable/dist/harness/events.d.ts`, `.../harness/view.d.ts`, `.../types.d.ts` (EntryRecord `:290-305`) and `.../pi-ai/dist/types.d.ts` (message shapes).

#### `client.ts`: NDJSON protocol-v1 client

```ts
export interface SocketLike {
  write(line: string): void; // already newline-terminated
  end(): void; // idempotent
  onData(handler: (chunk: string) => void): Disposable; // UTF-8 decoded chunks of ARBITRARY size/boundaries
  onClose(handler: (error: Error | null) => void): Disposable;
}
export interface SocketFactory { connect(path: string): Promise<SocketLike> } // rejects when the socket cannot be opened
export const PROTOCOL = 1; export const CLIENT_NAME = "cpd-web";

export type HostFrame =
  | { type: "welcome"; protocol: number; host_version: string; pi_durable: string; session: string; role: string; epoch: string; client_id: string; cwd: string; limits: { client_line: number } }
  | { type: "attached"; req: string; mode: "snapshot" | "resume"; epoch: string; from_seq: number; head_seq: number; client_id: string }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "result"; req: string; data: Record<string, unknown> }
  | { type: "error"; req?: string; code: string; message: string; details?: unknown }
  | { type: "host_state"; state: "idle" | "working" | "waiting" | "blocked"; name?: string; clients: number; held_ui: number; agent?: { model?: { provider: string; model_id: string }; thinking_level?: string | null } }
  | { type: "ui_request"; id: string; purpose: string; created_at: number; expires_at?: number | null; tool_call_id?: string; replayed?: boolean; method: string; [field: string]: unknown }
  | { type: "ui_resolved"; id: string; outcome: DialogOutcome; by?: string }
  | { type: "notice"; level: "info" | "warning" | "error"; message: string }
  | { type: "status"; key: string; text: string | null }
  | { type: "pong"; req?: string }
  | { type: "bye"; reason: "shutdown" | "restart" | "storage_failed" | "slow_client" | "detached"; reconnect: boolean };
export function parseHostFrame(line: string): HostFrame; // JSON.parse + discriminator check; failure -> WebError("protocol_error", ..., {line: first 200 chars})

export interface HostCursor { epoch: string; seq: number } // host cursor (distinct from the domain `Cursor`)
export interface HostClient {
  readonly welcome: Extract<HostFrame, { type: "welcome" }>;
  attach(cursor?: HostCursor): Promise<Extract<HostFrame, { type: "attached" }>>; // sends attach {req, conversation:"root", cursor?}
  submit(input: { text: string; whenBusy: "followUp" | "steer"; requestId?: string; images?: { mimeType: ImageMime; dataBase64: string }[] }): Promise<{ submissionId: number; status: string }>;
  compact(instructions?: string): Promise<{ taskId: number }>;
  configure(change: { model: string } | { thinkingLevel: string | null }): Promise<Record<string, unknown>>;
  listModels(): Promise<{ provider: string; model_id: string; name: string; thinking_levels: string[]; context_window: number }[]>;
  abort(): Promise<void>; // abort {background:false}
  abortSubmission(submissionId: number): Promise<{ outcome: unknown }>;
  respond(id: string, answer: { value: string } | { confirmed: boolean } | { answer: object } | { cancelled: true }): Promise<void>;
  detach(): Promise<void>;
  onFrame(handler: (frame: HostFrame) => void): Disposable; // live frames: snapshot, events, host_state, ui_request, ui_resolved, notice, status, bye
  onClose(handler: (error: Error | null) => void): Disposable;
  close(): void; // idempotent
}
export function connectHost(socketPath: string, deps: { factory: SocketFactory; scheduler: Scheduler }): Promise<HostClient>;
```

Wire shapes cited: client frames `protocol.ts:39-53` (scout-durable §2): `hello {protocol, min_protocol, client, name?}`, `attach {req, cursor?:{epoch,seq}, conversation?}`, `submit {req, text, when_busy?, request_id?, images?:[{mime_type, data}]}`, `compact {req, instructions?}`, `configure {req, model? | thinking_level?}` (exactly one), `list_models {req}`, `abort {req, background}`, `abort_submission {req, submission_id}`, `ui_response {req, id, value|confirmed|answer|cancelled}`, `detach {req}`; `req` is a client-chosen string, replies `result`/`error` carry it. Result shapes: scout-durable §3 (`submit` -> `{submission_id, status}`, `compact` -> `{task_id}`, `configure` -> `{agent}`, `list_models` -> `{models}`, `abort_submission` -> `{outcome}`).

Behaviours:

- `connectHost` opens the socket through `factory.connect` (rejection -> `host_unreachable`), writes `{"type":"hello","protocol":1,"min_protocol":1,"client":"cpd-web"}\n`, and waits for `welcome`; no `welcome` within 5000 ms of the Scheduler (`LIMITS.helloMs`, protocol.ts:6) or an `error` frame -> `host_unreachable` / `host_error`; `welcome.pi_durable !== "1.3.0"` or `protocol !== 1` -> `unsupported_host_version` (details `{piDurable, protocol}`; A3). The hello handshake sends protocol and client name exactly as above.
- Frames are split on `\n`; a frame may be split across any number of chunks, and several frames may arrive in one chunk; a trailing partial line is buffered. An empty line is ignored. A line failing `parseHostFrame` -> the client emits close with a `protocol_error` and ends the socket.
- Every command gets a fresh `req` (`r<n>`); the returned promise settles on the `result`/`error` with that `req`: `error` -> `WebError("host_error", message, {hostCode: code, details})`. Frames with no matching pending request are delivered to `onFrame`.
- `attach(cursor)` sends the cursor so the host may resume (`mode: "resume"` when `cursor.epoch` matches and `base.seq <= cursor.seq <= head`, scout-durable §4); the `attached` frame is returned and the snapshot/events frames that follow arrive through `onFrame`.
- `submit` carries `when_busy` and `images` as `{mime_type, data}` (images omitted when none) and `request_id` when given.
- `abort` sends `{"type":"abort","req":..,"background":false}`; `abortSubmission` sends `abort_submission {submission_id}`.
- Socket close (or `bye`) while the client is open: every pending request rejects `host_disconnected`, `onClose` fires once with `host_disconnected`. `close()` ends the socket and rejects pending requests with `host_disconnected`; after it nothing is emitted.

#### `frames.ts`: the single dialect seam (pi-durable 1.3.0)

```ts
export interface DurableTranscript { readonly items: TranscriptItem[]; readonly facts: DurableFacts }
export interface DurableFacts { model?: string /* provider/modelId */; thinking?: string | null; costUsd: number; lastContextTokens?: number; queued: { id: number; mode: string }[]; cwd?: string }
export interface DialectState { /* opaque, owned by frames.ts: live item bookkeeping */ }

export const SUPPORTED_PI_DURABLE = "1.3.0";
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void; // else WebError("unsupported_host_version")
export function createDialect(): Dialect;
export interface Dialect {
  applySnapshot(frame: { snapshot: unknown }): { transcript: DurableTranscript };
  applyEvents(frame: { events: unknown[] }): DialectOutput; // throws protocol_error on an unknown shape
  readonly transcript: DurableTranscript; // the current normalised transcript (the server-side cache)
}
export interface DialectOutput { append: TranscriptItem[]; update: TranscriptItem[]; deltas: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }[]; busy?: boolean; queued?: { id: number; mode: string }[]; agent?: { model?: string; thinking?: string | null }; replaced?: boolean /* a `snapshot` event inside events: the transcript was rebuilt, send a new tail window */ }
export function dialogFromRequest(req: Extract<HostFrame, { type: "ui_request" }>, transcript: readonly TranscriptItem[]): Dialog | { kind: "editor"; id: string } | { kind: "unsupported"; id: string; method: string; purpose: string }; // 3.8 table
```

The 1.3.0 shapes (read, `events.d.ts`): the `snapshot` frame's `snapshot` is a `SnapshotEvent {type:"snapshot", entries: EntryRecord[], tools, nestedTools, compactions, inbox: {id, mode}[], agent, usage, run?, generation?}`; each `events` frame's `events` is an array of `AgentEvent` (`type` one of `snapshot, run_start, run_end, turn_start, turn_end, message_start, message_update, message_end, tool_execution_start, tool_execution_update, tool_execution_end, inbox_update, submission, auto_retry_start, auto_retry_end, deferred_poll, entry_appended, agent_changed, usage_changed, task_failed, compaction_start, compaction_end`); `message_update.changes` are `MessageChange` (`text_start|thinking_start|toolcall_start` with `contentIndex` and `block`; `text_delta|thinking_delta` with `contentIndex` and `delta`; `toolcall_delta`; `block`; `message`). `EntryRecord = {id, conversationId, kind, model?: Message[], data?, head?, edits?, byTaskId?}`. The host forwards the snapshot and event batches unchanged (`pi/durable-host/run.ts:441,552-554`, scout-durable §5).

**Entry normalisation** (`applySnapshot` over `entries`, and the `entry` of `message_end` / `tool_execution_end` / `entry_appended`), by `EntryRecord.kind`:

| kind | Result |
|---|---|
| `pi.user` | `UserItem` (`u:<id>`) from `model[0]` (`UserMessage`: `content` string, or `TextContent`/`ImageContent` blocks; text blocks joined with `"\n"`, image blocks -> `TranscriptAttachment {kind:"image", mimeType, dataBase64}`) |
| `pi.assistant` | `AssistantItem` (`a:<id>`, `streaming:false`) from `model[0]` (`AssistantMessage`): `text` -> text block, `thinking` -> thinking block (kept separate, in order), `toolCall` -> a `ToolItem` (`tool:<callId>`, `args = arguments`, `status:"running"`) appended right after the assistant item; `usage.cost.total` is added to `facts.costUsd`; `usage.totalTokens` of the LAST assistant entry is `facts.lastContextTokens`; `model` -> `AssistantItem.model` |
| `pi.tool-result` | pairs with the `ToolItem` whose `callId === model[0].toolCallId`: sets `result {text, isError, durationMs}` and `status` (`done` / `failed`), and `diff` from `details.patch ?? details.diff` for `name === "edit"`. Result with no call in the transcript -> `OrphanResultItem` (`orphan:<callId>`) |
| `pi.compaction` | `CompactionItem` (`c:<id>`), `summary` = text of `model[0]` |
| `pi.system`, `pi.reset` | dropped |
| `cpd-context`, `cpd-plan-summary`, `cpd-plan-decision`, `cpd-skills` (host-written, `model: [{role:"user", content}]`, `data` = details; `pi/durable-host/plan-review.ts:102-113`, `turn.ts:88`) | `FoldItem` (`f:<id>`), `body` = text of `content`, title/defaultOpen by the rules of 3.3 (`data` plays `details`) |
| `cpd-chat`, `cpd-event`, `cpd-first-message`, `cpd-error`, `cpd-plan-review`, any other kind | dropped |

An entry whose `kind` is a `pi.*` kind but whose `model` is missing, empty or not a message of the expected role -> `protocol_error`. An `EntryRecord` without `id`/`kind` -> `protocol_error`. Whether `cpd-context` entries carry `data.summary` in the durable path is unverified (O14); the title then falls back to the first body line.

**Streaming** (`message_update.changes` for the in-flight assistant message, tracked per message between `message_start` and `message_end`):

- `message_start` of an `assistant` role message creates a live `AssistantItem` `live:<n>` (`streaming:true`, `blocks: []`) in `append`.
- `text_start`/`thinking_start` at `contentIndex` creates a block of that type and maps `contentIndex -> blocks.length`; `text_delta`/`thinking_delta` at `contentIndex` emits one `delta {itemId, blockIndex: mapped index, blockType, text: delta}` and also appends to the cached item. `toolcall_start` creates a running `ToolItem` (`tool:<block.id>`, args `{}`) in `append`; `toolcall_delta` is ignored (arguments are complete in `message_end`).
- `block` replaces the content at `contentIndex` (text/thinking block text replaced, via `update` of the whole live item); `message` replaces the whole live message (via `update`).
- `message_end {entry}`: the live item is replaced by the normalised entry, KEEPING the live id (`live:<n>`), `streaming:false`, in `update`; tool items of the entry are `update`d/appended by `tool:<callId>`. The cache keeps the live id so deltas and later updates address one item.
- `tool_execution_start {toolCallId, args}` updates (or appends) the tool item with `args`; `tool_execution_end {entry}` pairs the result as above; `tool_execution_end` without `entry` marks the tool item `failed` with `result {text:"", isError:true}`.
- `agent_changed {agent}` -> `DialectOutput.agent` (`agent.model` is `{provider, modelId}` here in pi-durable types; mapped to `provider/modelId`); `run_start` -> `busy:true`; `run_end` -> `busy:false`; `inbox_update {items}` and `snapshot.inbox` -> `queued` (`{id, mode}` of pi-durable `QueuedItem`); `usage_changed` is ignored (cost comes from entries).
- A `snapshot` event inside `events` (a rebuild) -> `applySnapshot` semantics and `replaced:true`.
- An event whose `type` is not in the list above, a `message_update` change whose `type` is not in the list above, or a non-object event -> `WebError("protocol_error", "unknown event <type>")`. Known events that Phase 1 does not use (`turn_start`, `turn_end`, `submission`, `auto_retry_*`, `deferred_poll`, `task_failed`, `compaction_start/end`, `tool_execution_update`) are accepted and produce no output.

Behaviours to test: user and assistant messages normalise to items; thinking blocks stay separate from text, in order; tool calls pair with results; a result without its call is an orphan; streaming text deltas append to the assistant item; cpd custom entries map to folds (default open only for `cpd-plan-summary`); compaction maps to a compaction item; an unknown frame shape raises `protocol_error`; `assertSupportedHost({pi_durable:"1.5.0",...})` raises `unsupported_host_version`.

#### `backend.ts`: `SessionBackend` for durable sessions

```ts
// SessionBackend, SessionHandle, SessionEmit, ResolvedSession and GitBoundary are the shared interfaces of 6.1 (runtime.ts)
export function createDurableBackend(deps: { connect: (socketPath: string) => Promise<HostClient>; cpd: Cpd; git: GitBoundary; scheduler: Scheduler; windowSize?: number }): SessionBackend;
```

Behaviours:

- `attach`: connects (`deps.connect(session.socketPath)`; a missing `socketPath` -> `host_unreachable`), calls `client.attach()`, builds the transcript with a `Dialect` from the first `snapshot` frame (resume is not used in Phase 1: every attach is a snapshot because the browser holds no host cursor), then emits in order `session.attached` (capabilities of section 8, `title` = `ref.title`, `meta {ref, footer}`), `session.window {mode:"tail"}` = the last `windowSize` (default `WINDOW_SIZE`) items of the cached transcript with `hasEarlier = items.length > windowSize` and `cursor = "dur:<index of the first returned item>"` (null when none earlier), `session.footer`, `session.status`, and one `session.dialog` per replayed held `ui_request` (the host replays them with `replayed:true` after the frames, scout-durable §4).
- Server-side windowing (A5): the host sends the full snapshot; the backend caches the normalised transcript and serves windows from it. `loadEarlier("dur:<i>")` returns the `windowSize` items ending before index `i` as `session.window {mode:"earlier"}` with a new cursor (null at the start); an unparsable cursor -> `invalid_message`. Items past index `i` are never repeated.
- Live: each `events` frame goes through `dialect.applyEvents` and is emitted as `session.append` / `session.update` / `session.delta` (deltas in order), plus `session.status {busy, queued}` when they changed. A `replaced` output emits a fresh `session.window tail`. `host_state` frames update `busy` (busy = `state !== "idle"`; the host sets only `idle|working|blocked`, scout-durable §3) and the agent.
- `submit`: empty `text.trim()` with no images -> `invalid_submission`; `client.submit({text, whenBusy, requestId: clientSubmissionId, images})`; then `session.submitAck {clientSubmissionId, queued: busy, submissionId}`. When the session is busy the submission is added to `queued` (`{clientSubmissionId, submissionId, text, whenBusy}`) and is removed when the dialect reports the inbox without that submission id (queued submissions are tracked until their submission completes); `session.status` is emitted on each change. A host `error` becomes `session.error {clientSubmissionId, error: host_error}` (and the promise rejects with it).
- `abort` -> `client.abort()`. `abortSubmission(clientSubmissionId)` looks up the `submissionId` of that queued submission (unknown -> `invalid_message`) and sends `abort_submission`; the queued entry is removed.
- `compact(instructions)` -> `client.compact`. `listModels` -> `client.listModels()` mapped to `ModelChoice[]` (`model = provider + "/" + model_id`) -> `session.models`.
- `configure`: `{kind:"model"}` -> `client.configure({model})`; `{kind:"thinking"}` -> `client.configure({thinkingLevel})`; `{kind:"tier"}` -> `cpd session retier <id> --tier <tier>` via `deps.cpd` (`sessionRetierArgs`); host refusal `refused "a run is pending"` (scout-durable §7.4) surfaces as `host_error` with `details.hostCode: "refused"`. After a model/thinking change the footer is recomputed and `session.footer` emitted.
- Footer: `model` from host `agent.model` (`provider/model_id`), `thinking` from `agent.thinking_level`, `tier` from `ref.tier`, `cost` = `facts.costUsd` (sum of `usage.cost.total` of the active entries; compaction drops earlier cost, a documented limitation), `contextPercent` = `round(lastContextTokens / contextWindow * 100)` where `contextWindow` is `context_window` of the current model in `list_models` (fetched once at attach; an approximation, not pi-core's footer formula, O8), `branch` = `git.branch(cwd)` with `cwd` from `welcome.cwd`. A missing input gives `{state:"unknown", reason}`.
- Dialogs: a `ui_request` becomes `session.dialog {dialog: {...Dialog, id, interactive:true}}` via `dialogFromRequest`; a `ui_resolved` frame becomes `session.dialogResolved`; `respond(dialogId, response)` maps per the table of 3.8 (`ask_user_question answered` -> `ui_response {id, answer: result}`; `cancelled` -> `{cancelled:true}`; plan review as in 3.8; an editor `ui_request` that follows a `Change` is answered by the backend; `reject` runs `cpd plan decide` via `deps.cpd`). A response for an unknown dialog id -> `invalid_message`. Cancel (host keeps the request held, scout-durable §7.5) emits `session.dialogResolved {outcome:"cancelled"}` when the host's `ui_resolved` arrives.
- Socket close (`onClose`/`bye`) -> `session.error {sessionId, error: host_disconnected}`; the handle then rejects every method with `host_disconnected` until disposed. `dispose()` detaches (`client.detach()` best effort), closes the client and drops every listener.
