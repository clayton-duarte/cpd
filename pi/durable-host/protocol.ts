export const PROTOCOL_VERSIONS = [1] as const;
export const LIMITS = {
  clientLine: 1048576,
  hostLine: 67108864,
  slowClient: 8388608,
  helloMs: 5000,
  rebaseAtRunEnd: 256,
  rebaseMax: 4096,
} as const;

export type Cursor = { epoch: string; seq: number };

/** The agent state a `configure` answers and `host_state.agent` carries (contract §6.17). */
export type AgentState = { model?: { provider: string; model_id: string }; thinking_level: string };

/** The `history` page sizes and the images of a `submit` (contract §6.17). */
export const HISTORY_LIMIT = 50;
export const HISTORY_MAX = 500;
export const IMAGE_MAX_COUNT = 4;
export const IMAGE_MAX_TOTAL = 786432;
export const IMAGE_MIME = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;

export type ErrorCode =
  | "protocol_mismatch" | "hello_timeout" | "bad_frame" | "frame_too_large"
  | "unknown_type" | "bad_args" | "not_attached" | "busy" | "unsupported"
  | "storage_failed" | "shutting_down" | "internal" | "refused"
  | "hello_required" | "ui_unknown";

export class ProtocolError extends Error {
  code: ErrorCode;
  details?: object;
  constructor(code: ErrorCode, message: string, details?: object) {
    super(message);
    this.code = code;
    this.details = details;
  }
}

export type ToHost =
  | { type: "hello"; protocol: number; min_protocol: number; client: string; name?: string }
  | { type: "attach"; req: string; cursor?: Cursor; conversation?: string }
  | { type: "submit"; req: string; text: string; when_busy: "followUp" | "steer" | "reject"; request_id?: string; images?: readonly { mime_type: string; data: string }[] }
  | { type: "compact"; req: string; instructions?: string }
  | { type: "configure"; req: string; model?: string; thinking_level?: string | null }
  | { type: "list_models"; req: string }
  | { type: "fork"; req: string; at: number; ownership?: "lead" | "manager" }
  | { type: "history"; req: string; before?: string; limit?: number }
  | { type: "abort"; req: string; background: boolean }
  | { type: "abort_submission"; req: string; submission_id: number }
  | { type: "ping"; req?: string }
  | { type: "detach"; req: string }
  | { type: "ui_response"; req: string; id: string; value?: string; confirmed?: boolean; answer?: object; cancelled?: boolean };

export type HostState = {
  state: "idle" | "working" | "waiting" | "blocked";
  name?: string;
  agent?: unknown;
  held_ui: number;
};

export type UiPurpose = "ask_user_question" | "plan_review" | "secret_approve" | "host_approve" | "mcp_login" | "other";
export type UiMethod = "select" | "confirm" | "input" | "editor" | "questionnaire";

export type UiQuestion = {
  question: string;
  header: string;
  multiSelect?: boolean;
  options: { label: string; description: string; preview?: string }[];
};

export type UiRequestBase = {
  type: "ui_request";
  id: string;
  purpose: UiPurpose;
  created_at: number;
  expires_at?: number;
  tool_call_id?: string;
  replayed?: boolean;
};

export type ToClient =
  | { type: "welcome"; protocol: number; host_version: string; pi_durable: string; session: string; role: "lead" | "manager"; epoch: string; client_id: string; cwd: string; limits: { client_line: number } }
  | { type: "attached"; req: string; mode: "snapshot" | "resume"; epoch: string; from_seq: number; head_seq: number; client_id: string }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "result"; req: string; data: object }
  | { type: "error"; req?: string; code: ErrorCode; message: string; details?: object }
  | { type: "host_state"; state: HostState["state"]; name?: string; clients: number; held_ui: number; agent?: unknown }
  | (UiRequestBase & { method: "select"; title: string; options: string[]; timeout?: number })
  | (UiRequestBase & { method: "confirm"; title: string; message: string; timeout?: number })
  | (UiRequestBase & { method: "input"; title: string; placeholder?: string; timeout?: number })
  | (UiRequestBase & { method: "editor"; title: string; prefill?: string })
  | (UiRequestBase & { method: "questionnaire"; questions: UiQuestion[] })
  | { type: "ui_resolved"; id: string; outcome: "answered" | "cancelled" | "timeout" | "superseded" | "aborted"; by?: string }
  | { type: "notice"; level: "info" | "warning" | "error"; message: string }
  | { type: "status"; key: string; text: string | null }
  | { type: "pong"; req?: string }
  | { type: "bye"; reason: "shutdown" | "restart" | "storage_failed" | "slow_client" | "detached"; reconnect: boolean };

type Parsed =
  | { ok: true; msg: ToHost }
  | { ok: false; error: { code: "bad_frame" | "unknown_type" | "bad_args"; message: string; req?: string } };

class ArgError extends Error {}

const bad = (field: string, want: string): never => {
  throw new ArgError(`${field} must be ${want}`);
};
const str = (o: Record<string, unknown>, k: string): string => (typeof o[k] === "string" ? (o[k] as string) : bad(k, "a string"));
const int = (o: Record<string, unknown>, k: string): number => (Number.isInteger(o[k]) ? (o[k] as number) : bad(k, "an integer"));
const optStr = (o: Record<string, unknown>, k: string): string | undefined => (o[k] === undefined ? undefined : str(o, k));

const KINDS = new Set(["hello", "attach", "submit", "compact", "configure", "list_models", "fork", "history", "abort", "abort_submission", "ping", "detach", "ui_response"]);
const WHEN_BUSY = ["followUp", "steer", "reject"] as const;
const OWNERSHIP = ["lead", "manager"] as const;
const BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

/** The `submit.images` of contract §6.17: an empty array is the absent one; `undefined` when absent. */
function images(o: Record<string, unknown>): readonly { mime_type: string; data: string }[] | undefined {
  const raw = o.images;
  if (raw === undefined) return undefined;
  if (!Array.isArray(raw)) bad("images", "an array");
  if (raw.length === 0) return undefined;
  if (raw.length > IMAGE_MAX_COUNT) bad("images", `at most ${IMAGE_MAX_COUNT} images`);
  const out: { mime_type: string; data: string }[] = [];
  let total = 0;
  for (const item of raw) {
    if (typeof item !== "object" || item === null || Array.isArray(item)) bad("images", "an array of objects");
    const im = item as Record<string, unknown>;
    const mime = im.mime_type;
    if (typeof mime !== "string" || !(IMAGE_MIME as readonly string[]).includes(mime)) bad("mime_type", `one of ${IMAGE_MIME.join(", ")}`);
    const data = im.data;
    if (typeof data !== "string" || !BASE64.test(data)) bad("data", "an RFC 4648 base64 string with its padding");
    total += data.length;
    out.push({ mime_type: mime, data });
  }
  if (total > IMAGE_MAX_TOTAL) bad("images", `at most ${IMAGE_MAX_TOTAL} bytes of base64 data together`);
  return out;
}

function uiAnswer(o: Record<string, unknown>): { value?: string; confirmed?: boolean; answer?: object; cancelled?: boolean } {
  const fields = ["value", "confirmed", "answer", "cancelled"] as const;
  const present = fields.filter((k) => o[k] !== undefined);
  if (present.length !== 1) bad("ui_response", "exactly one of value, confirmed, answer or cancelled");
  const k = present[0];
  if (k === "value") return { value: str(o, "value") };
  if (k === "confirmed") {
    if (typeof o.confirmed !== "boolean") bad("confirmed", "a boolean");
    return { confirmed: o.confirmed as boolean };
  }
  if (k === "answer") {
    const a = o.answer;
    if (typeof a !== "object" || a === null || Array.isArray(a)) bad("answer", "an object");
    return { answer: a as object };
  }
  if (typeof o.cancelled !== "boolean") bad("cancelled", "a boolean");
  return { cancelled: o.cancelled as boolean };
}

function build(o: Record<string, unknown>, type: string): ToHost {
  if (type === "hello") {
    const name = optStr(o, "name");
    return { type, protocol: int(o, "protocol"), min_protocol: int(o, "min_protocol"), client: str(o, "client"), ...(name === undefined ? {} : { name }) };
  }
  if (type === "ping") {
    const req = optStr(o, "req");
    return req === undefined ? { type } : { type, req };
  }
  const req = str(o, "req");
  switch (type) {
    case "attach": {
      const conversation = optStr(o, "conversation");
      const extra = conversation === undefined ? {} : { conversation };
      const c = o.cursor;
      if (c === undefined) return { type, req, ...extra };
      if (typeof c !== "object" || c === null || Array.isArray(c)) return bad("cursor", "an object");
      const cur = c as Record<string, unknown>;
      return { type, req, cursor: { epoch: str(cur, "epoch"), seq: int(cur, "seq") }, ...extra };
    }
    case "submit": {
      const text = str(o, "text");
      const wb = o.when_busy === undefined ? "followUp" : (WHEN_BUSY as readonly unknown[]).includes(o.when_busy) ? (o.when_busy as (typeof WHEN_BUSY)[number]) : bad("when_busy", "followUp, steer or reject");
      const request_id = optStr(o, "request_id");
      const imgs = images(o);
      return { type, req, text, when_busy: wb, ...(request_id === undefined ? {} : { request_id }), ...(imgs === undefined ? {} : { images: imgs }) };
    }
    case "compact": {
      const instructions = optStr(o, "instructions");
      return { type, req, ...(instructions === undefined ? {} : { instructions }) };
    }
    case "configure": {
      const hasModel = o.model !== undefined;
      const hasLevel = o.thinking_level !== undefined;
      if (hasModel === hasLevel) bad("model|thinking_level", "exactly one of model and thinking_level");
      if (hasModel) return { type, req, model: str(o, "model") };
      if (o.thinking_level !== null && typeof o.thinking_level !== "string") bad("thinking_level", "a string or null");
      return { type, req, thinking_level: o.thinking_level as string | null };
    }
    case "list_models":
      return { type, req };
    case "fork": {
      const ownership = o.ownership;
      if (ownership !== undefined && !(OWNERSHIP as readonly unknown[]).includes(ownership)) bad("ownership", "lead or manager");
      return { type, req, at: int(o, "at"), ...(ownership === undefined ? {} : { ownership: ownership as (typeof OWNERSHIP)[number] }) };
    }
    case "history": {
      const before = optStr(o, "before");
      const limit = o.limit;
      if (limit !== undefined && (!Number.isInteger(limit) || (limit as number) < 1)) bad("limit", "a positive integer");
      if (limit !== undefined && (limit as number) > HISTORY_MAX) bad("limit", `a positive integer no greater than ${HISTORY_MAX}`);
      return { type, req, ...(before === undefined ? {} : { before }), ...(limit === undefined ? {} : { limit: limit as number }) };
    }
    case "abort": {
      if (o.background !== undefined && typeof o.background !== "boolean") bad("background", "a boolean");
      return { type, req, background: o.background === true };
    }
    case "abort_submission":
      return { type, req, submission_id: int(o, "submission_id") };
    case "ui_response":
      return { type, req, id: str(o, "id"), ...uiAnswer(o) };
    default:
      return { type: "detach", req };
  }
}

export function parseLine(line: string): Parsed {
  let v: unknown;
  try {
    v = JSON.parse(line);
  } catch {
    return { ok: false, error: { code: "bad_frame", message: "line is not JSON" } };
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return { ok: false, error: { code: "bad_frame", message: "line is not a JSON object" } };
  const o = v as Record<string, unknown>;
  if (typeof o.type !== "string") return { ok: false, error: { code: "bad_frame", message: "type must be a string" } };
  const req = typeof o.req === "string" ? o.req : undefined;
  const keep = req === undefined ? {} : { req };
  if (!KINDS.has(o.type)) return { ok: false, error: { code: "unknown_type", message: `unknown type ${o.type}`, ...keep } };
  try {
    return { ok: true, msg: build(o, o.type) };
  } catch (e) {
    if (!(e instanceof ArgError)) throw e;
    return { ok: false, error: { code: "bad_args", message: e.message, ...(o.type === "hello" ? {} : keep) } };
  }
}

export function encode(msg: ToClient): string {
  return `${JSON.stringify(msg)}\n`;
}

export function negotiate(hello: Extract<ToHost, { type: "hello" }>): number | undefined {
  return [...PROTOCOL_VERSIONS].reverse().find((v) => v >= hello.min_protocol && v <= hello.protocol);
}
