import {
  WebError,
  type Dialog,
  type Question,
  type TranscriptItem,
} from "../../protocol/domain.ts";
import type { HostFrame } from "./client.ts";

// The single dialect seam: pi-durable 1.5.0 `view` / `ops` frames (STUB: red-test branch, not implemented) -> domain transcript (CONTRACT 6.4).

export interface DurableFacts {
  model?: string;
  thinking?: string | null;
  busy: boolean;
  costUsd: number;
  lastContextTokens?: number;
  queued: { id: number; mode: string }[];
  cwd?: string;
}
export interface DurableTranscript {
  readonly items: TranscriptItem[];
  readonly facts: DurableFacts;
  readonly before?: number;
}

export const SUPPORTED_PI_DURABLE = "1.3.0";
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void {
  if (welcome.pi_durable !== SUPPORTED_PI_DURABLE || welcome.protocol !== 1) {
    throw new WebError(
      "unsupported_host_version",
      `durable host pi-durable ${welcome.pi_durable} protocol ${welcome.protocol}; only pi-durable ${SUPPORTED_PI_DURABLE} protocol 1 is supported`,
      { piDurable: welcome.pi_durable, protocol: welcome.protocol },
    );
  }
}

export interface DialectOutput {
  append: TranscriptItem[];
  update: TranscriptItem[];
  deltas: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }[];
  busy?: boolean;
  queued?: { id: number; mode: string }[];
  agent?: { model?: string; thinking?: string | null };
  replaced?: boolean;
}
export interface Gap {
  readonly after: number;
  readonly upTo: number;
}
export interface Dialect {
  applyView(frame: { view: unknown }): { transcript: DurableTranscript };
  applyOps(frame: { ops: unknown[] }): DialectOutput;
  applyHistory(frame: { entries: unknown[]; next?: string }): void;
  readonly gap: Gap | undefined;
  readonly transcript: DurableTranscript;
}

type Obj = Record<string, unknown>;
const protocolError = (message: string): never => {
  throw new WebError("protocol_error", message);
};
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const obj = (v: unknown, what: string): Obj => (isObj(v) ? v : protocolError(`${what} is not an object`));
const str = (v: unknown, what: string): string => (typeof v === "string" ? v : protocolError(`${what} is not a string`));
const arr = (v: unknown, what: string): unknown[] => (Array.isArray(v) ? v : protocolError(`${what} is not an array`));

const notImplemented = (): never => {
  throw new Error("not implemented");
};

export function createDialect(): Dialect {
  const transcript: DurableTranscript = { items: [], facts: { busy: false, costUsd: 0, queued: [] } };
  return {
    transcript,
    gap: undefined,
    applyView: notImplemented,
    applyOps: notImplemented,
    applyHistory: notImplemented,
  };
}

function questionsOf(raw: unknown): Question[] {
  return arr(raw, "questions").map((q) => {
    const o = obj(q, "question");
    const options = arr(o.options, "question options").map((opt) => {
      const x = obj(opt, "question option");
      return { label: str(x.label, "option label"), description: str(x.description ?? "", "option description"), ...(typeof x.preview === "string" ? { preview: x.preview } : {}) };
    });
    return { header: str(o.header, "question header"), question: str(o.question, "question text"), options, ...(o.multiSelect === true ? { multiSelect: true } : {}) };
  });
}

export function dialogFromRequest(
  req: Extract<HostFrame, { type: "ui_request" }>,
  transcript: readonly TranscriptItem[],
): Dialog | { kind: "editor"; id: string } | { kind: "unsupported"; id: string; method: string; purpose: string } {
  if (req.method === "questionnaire") {
    return { id: req.id, method: "ask_user_question", questions: questionsOf(req.questions), interactive: true };
  }
  if (req.method === "select" && req.purpose === "plan_review" && JSON.stringify(req.options) === '["Approve","Change"]') {
    let title = "Plan review";
    let summary = "";
    for (const item of [...transcript].reverse()) {
      if (item.kind === "tool" && item.name === "cpd_plan_review" && typeof item.args.title === "string") {
        title = item.args.title;
        break;
      }
    }
    for (const item of [...transcript].reverse()) {
      if (item.kind === "fold" && item.foldKind === "cpd-plan-summary") {
        summary = item.body;
        break;
      }
    }
    return { id: req.id, method: "cpd_plan_review", title, summary, interactive: true };
  }
  if (req.method === "editor") return { kind: "editor", id: req.id };
  return { kind: "unsupported", id: req.id, method: req.method, purpose: req.purpose };
}
