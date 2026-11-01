// The typed wire surface of the cpd web client, per docs/research/web-client-api.md §1.
// Every still-open P1.6/P1.8 shape carries the @open(P1.6) / @open(P1.8) marker and is
// never written as a decided shape (A5, hs-n596). The pin samples below throw until the
// impl Job fills them in — this is what makes the wire tests RED.

/** A cursor: "I applied every frame up to seq of that epoch" (protocol.ts:11, §1.1; §5.4). */
export interface Cursor {
  epoch: string;
  seq: number;
}

/**
 * Error codes (protocol.ts:13-17 and §1.1). `ui_unknown` — not `ui_resolved` — is the
 * late-answer error code (durable-leads 4.4 rule 3, §1.1 row 22).
 */
export type ErrorCode =
  | "protocol_mismatch"
  | "hello_timeout"
  | "hello_required"
  | "bad_frame"
  | "frame_too_large"
  | "unknown_type"
  | "bad_args"
  | "not_attached"
  | "busy"
  | "ui_unknown"
  | "unsupported"
  | "storage_failed"
  | "shutting_down"
  | "internal"
  | "refused";

/** The v1 H→C frames (§1.1; protocol.ts:41-50). */
export type ToClient =
  | WelcomeFrame
  | AttachedFrame
  | SnapshotFrame
  | EventsFrame
  | ResultFrame
  | ErrorFrame
  | HostStateFrame
  | PongFrame
  | ByeFrame;

export interface WelcomeFrame {
  type: "welcome";
  protocol: number;
  host_version: string;
  pi_durable: string;
  session: string;
  role: "lead" | "manager";
  epoch: string;
  client_id: string;
  cwd: string;
  limits: { client_line: number };
}

export interface AttachedFrame {
  type: "attached";
  req: string;
  mode: "snapshot" | "resume";
  epoch: string;
  from_seq: number;
  head_seq: number;
  client_id: string;
}

/** A `snapshot` frame replaces the whole view (§5.3); its payload is a pi-durable `SnapshotEvent`, unchanged. */
export interface SnapshotFrame {
  type: "snapshot";
  epoch: string;
  seq: number;
  snapshot: unknown;
}

/** An `events` frame: one commit of `AgentEvent`, unchanged (§1.1, protocol.ts:45; §5.1). */
export interface EventsFrame {
  type: "events";
  epoch: string;
  seq: number;
  events: readonly unknown[];
}

export interface ResultFrame {
  type: "result";
  req: string;
  data: object;
}

export interface ErrorFrame {
  type: "error";
  req?: string;
  code: ErrorCode;
  message: string;
  details?: object;
}

export interface HostStateFrame {
  type: "host_state";
  state: "idle" | "working" | "waiting" | "blocked";
  name?: string;
  clients: number;
  held_ui: number;
  agent?: unknown;
}

export interface PongFrame {
  type: "pong";
  req?: string;
}

export interface ByeFrame {
  type: "bye";
  reason: "shutdown" | "restart" | "storage_failed" | "slow_client" | "detached";
  reconnect: boolean;
}

// ---------------------------------------------------------------------------
// P1.6 — ui_request / ui_response / ui_resolved (§1.2, web-ui.md 3.3).

export type UiPurpose =
  | "ask_user_question"
  | "plan_review"
  | "secret_approve"
  | "host_approve"
  | "mcp_login"
  | "other";

/** An authored option (pi/ask-user-question/tool/types.ts:47-62, row 23). */
export interface UiSelectOption {
  label: string;
  description: string;
  preview?: string;
}

/**
 * The `questionnaire` method's `questions` array — `@open(P1.6/P1.8)`, inferred from
 * `pi/ask-user-question/tool/types.ts:41-91` (row 23, C3). Never a decided shape.
 */
export interface UiQuestionnaireQuestion {
  question: string;
  header: string;
  multiSelect?: boolean;
  options: readonly UiSelectOption[];
}

export interface UiRequest {
  type: "ui_request";
  id: string;
  method: "select" | "confirm" | "input" | "editor" | "questionnaire";
  // the method's fields (pi's field names, rpc-types.d.ts:406-476; row 23):
  title?: string;
  message?: string;
  options?: readonly UiSelectOption[];
  placeholder?: string;
  prefill?: string;
  timeout?: number;
  /** @open(P1.6/P1.8) — the questionnaire fields; never a decided shape. */
  questions?: readonly UiQuestionnaireQuestion[];
  purpose: UiPurpose;
  created_at?: string;
  expires_at?: string;
  tool_call_id?: string;
  /** @open(P1.6/P1.8) — whether frames replay after host_state with replayed:true (C3). */
  replayed?: boolean;
}

/**
 * One answer (`value` | `confirmed` | `answer` | `cancelled`) of a held `ui_request` (§1.2, row 14).
 * The `answer` envelope is `@open(P1.6/P1.8)` (C3, hs-n596(1)); its contents are `QuestionnaireResult`.
 */
export interface UiResponse {
  type: "ui_response";
  req: string;
  id: string;
  value?: string;
  confirmed?: boolean;
  /** @open(P1.6/P1.8) — the questionnaire result envelope; contents recorded as QuestionnaireResult. */
  answer?: object;
  cancelled?: true;
}

export interface UiResolvedFrame {
  type: "ui_resolved";
  id: string;
  outcome: "answered" | "cancelled" | "timeout" | "superseded" | "aborted";
  by?: string;
}

/** The questionnaire result, recorded from pi/ask-user-question/tool/types.ts:109-157 (§1.2). */
export interface QuestionnaireResult {
  answers: readonly QuestionAnswer[];
  cancelled: boolean;
  globalNote?: string;
  unanswered?: number[];
  error?: string;
}

export interface QuestionAnswer {
  questionIndex: number;
  question: string;
  kind: "option" | "custom" | "multi";
  answer: string | null;
  selected?: string[];
  notes?: string;
  preview?: string;
}

// ---------------------------------------------------------------------------
// P1.8 — history / list_models / configure / fork / compact (§1.2, web-ui.md 3.3).

/** The C→H P1.8 requests. */
export type P18Request =
  | { type: "history"; req: string; before?: string; limit?: number }
  | { type: "list_models"; req: string }
  | {
      type: "configure";
      req: string;
      model?: { provider: string; model_id: string } | null;
      thinking_level?: ThinkingLevel | null;
    }
  | { type: "fork"; req: string; at: string; ownership?: string }
  | { type: "compact"; req: string; instructions?: string };

export type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

export interface ModelRef {
  provider: string;
  model_id: string;
  name: string;
  thinking_levels: readonly unknown[];
  context_window: number;
}

/** @open(P1.6/P1.8) — the `history` result shape (row 21 inferred, C4, hs-n596(1)). */
export interface HistoryResult {
  entries: readonly unknown[];
  next?: unknown;
}

/** @open(P1.6/P1.8) — the `list_models` result shape (row 21 inferred, hs-n596(1)). */
export interface ListModelsResult {
  models: readonly ModelRef[];
}

/** @open(P1.6/P1.8) — the `configure` result shape (row 21, C4). */
export interface ConfigureResult {
  agent: unknown;
}

/** @open(P1.6/P1.8) — the `fork` result shape (row 11, C4, hs-n596(1)). */
export interface ForkResult {
  conversation_id: string;
}

/** The `compact` (P1.8 row 6) result: a `TaskId<CompactionResult>` (read: types.d.ts:610). */
export interface CompactResult {
  task_id: string;
}

/** The decided `result.data` for `submit` (row 21, measured). */
export interface SubmitResult {
  submission_id: string;
  status: "queued" | "placed" | "done" | "unanswered";
}

/** The decided `result.data` for `abort_submission` (row 21). */
export interface AbortSubmissionResult {
  outcome: "aborted" | "already_placed" | "settled" | "not_found";
}

// ---------------------------------------------------------------------------
// Red-test pin samples. Each throws `not implemented` until the impl Job lands;
// the return types ARE the pinned wire contract, so the tests stay RED now.

/** The typed v1 H→C sample frames used by the wire pinning tests (STUB). */
export function v1HostSamples(): readonly ToClient[] {
  return [
    {
      type: "welcome",
      protocol: 1,
      host_version: "0.1.0",
      pi_durable: "0.1.0",
      session: "session-1",
      role: "lead",
      epoch: "e1",
      client_id: "client-1",
      cwd: "/tmp",
      limits: { client_line: LIMITS_CLIENT_LINE },
    },
    { type: "attached", req: "r1", mode: "snapshot", epoch: "e1", from_seq: 0, head_seq: 5, client_id: "client-1" },
    { type: "snapshot", epoch: "e1", seq: 5, snapshot: {} },
    { type: "events", epoch: "e1", seq: 6, events: [] },
    { type: "result", req: "r1", data: {} },
    { type: "error", req: "r1", code: "bad_args", message: "bad args" },
    { type: "host_state", state: "idle", clients: 1, held_ui: 0 },
    { type: "pong", req: "r1" },
    { type: "bye", reason: "shutdown", reconnect: false },
  ];
}

/** The v1 line limit reported in `welcome.limits` (protocol.ts:2-9). */
const LIMITS_CLIENT_LINE = 64 * 1024 * 1024;

/** The typed P1.6 sample frames — ui_request, ui_response, ui_resolved (STUB). */
export function p16UiSamples(): readonly (UiRequest | UiResponse | UiResolvedFrame)[] {
  return [
    { type: "ui_request", id: "u1", method: "confirm", title: "Approve?", message: "Apply the plan?", purpose: "plan_review" },
    { type: "ui_response", req: "r1", id: "u1", confirmed: true },
    { type: "ui_resolved", id: "u1", outcome: "answered", by: "client-1" },
  ];
}

/** The typed P1.8 result samples (STUB). */
export function p18ResultSamples(): readonly (HistoryResult | ListModelsResult | ConfigureResult | ForkResult | CompactResult)[] {
  return [
    { entries: [], next: undefined },
    { models: [] },
    { agent: {} },
    { conversation_id: "conversation-1" },
    { task_id: "task-1" },
  ];
}
