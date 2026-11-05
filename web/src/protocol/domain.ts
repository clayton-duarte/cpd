// Shared domain types (web/CONTRACT.md section 3). Pure types, constants and WebError.

export const WINDOW_SIZE = 40; // A5: tail and each loadEarlier page
export const IMAGE_MAX_COUNT = 4; // durable host limit, protocol.ts:19 (scout-durable §2)
export const IMAGE_MAX_TOTAL_BASE64 = 786_432; // sum of base64 `data.length`, protocol.ts:20 (scout-durable §2)
export const IMAGE_MIME_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const; // protocol.ts:21
export type ImageMime = (typeof IMAGE_MIME_TYPES)[number];
export const TEXT_INLINE_MAX_BYTES = 65_536; // Phase 1 choice: larger text files are rejected
export const PI_SEND_TEXT_MAX = 20_000; // `cpd session send` text is 1..20000 chars (scout-pi §6, `cpd session send --help`)
export const FOLD_TITLE_MAX = 160; // fold title cut, as pi/lib/render.ts oneLine (scout-pi §3)
export const TOOL_PREVIEW_LINES = 10; // compact-tools previewLines default (scout-pi §4)

export type Runner = "pi" | "durable"; // `runner` of `cpd session list` (scout-cockpit §4)
export type CompletionLabel = "safe_to_archive" | "unanswered_ask" | "open_work" | "unrecorded_decision" | "unescalated_issue";
export type CompletionTone = "done" | "decision" | "blocked";
export interface Completion {
  readonly label: CompletionLabel;
  readonly text: string;
  readonly score: number;
  readonly tone: CompletionTone;
}

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
  readonly blocks: readonly AssistantBlock[]; // order of the message `content`; toolCall blocks are NOT blocks (they are ToolItems). A message with no text or thinking block yields NO AssistantItem, only its ToolItems.
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

export type FoldKind = "cpd-context" | "cpd-plan-decision" | "cpd-plan-summary" | "cpd-skills" | "cpd";
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

export type Cursor = string; // opaque to the browser; echoed back in `session.loadEarlier`
// pi:   `pi:<byteOffset>`  the file offset of the first byte of the earliest loaded line
// durable: `dur:<index>`   the index into the server-side normalised transcript of the earliest loaded item

export interface Window {
  readonly items: readonly TranscriptItem[]; // oldest first
  readonly hasEarlier: boolean;
  readonly cursor: Cursor | null; // pass as `before` to load earlier; null exactly when hasEarlier is false
}

export type Stat<T> = { readonly state: "known"; readonly value: T } | { readonly state: "unknown"; readonly reason: string };

export interface FooterStats {
  readonly model: Stat<string>; // `provider/modelId`
  readonly tier: Stat<string>;
  readonly thinking: Stat<string>; // thinking level, e.g. "high"; "off" is a value
  readonly contextPercent: Stat<number>; // 0..100
  readonly cost: Stat<number>; // USD, sum of assistant usage.cost.total
  readonly branch: Stat<string>; // git branch of the session cwd
}

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
  constructor(code: ErrorCode, message: string, details?: Readonly<Record<string, unknown>>) {
    super(message);
    this.name = "WebError";
    this.code = code;
    if (details !== undefined) this.details = details;
  }
}
export interface WireError { readonly code: ErrorCode; readonly message: string; readonly details?: Readonly<Record<string, unknown>> } // WebError serialised
/** WebError -> as is; anything else -> {code:"internal", message}. */
export const toWireError = (e: unknown): WireError => {
  if (e instanceof WebError) return e.details === undefined ? { code: e.code, message: e.message } : { code: e.code, message: e.message, details: e.details };
  return { code: "internal", message: e instanceof Error ? e.message : String(e) };
};
