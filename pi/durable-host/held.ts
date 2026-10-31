// Contract §6.15 `held.ts`: the host's memory-only map of held UI requests (4.4 rule 6), and its pure questions.
// The types are real so the tests and the rest of the host type against them; the bodies are P1.6's tail, not this Job.

export type Held = {
  id: string;
  method: "select" | "confirm" | "input" | "editor" | "questionnaire";
  purpose: "ask_user_question" | "plan_review" | "secret_approve" | "host_approve" | "mcp_login" | "other";
  created_at: number;
  expires_at?: number;
  fields: object;
  signal?: AbortSignal;
  resolve(value?: object): void;
  reject(e: unknown): void;
};

export type AnswerValue = { value?: string; confirmed?: boolean; answer?: object; cancelled?: boolean };
export type Outcome = "answered" | "cancelled" | "timeout" | "superseded" | "aborted";
export type CreateOptions = {
  ms?: number;
  signal?: AbortSignal;
  toolCallId?: string;
  onResolve(outcome: Outcome, by: string): void;
};

/** The held map's functions, as the ports that use it see them (dialogs, tools, plan-review, the server's `answerUi`). */
export type HeldApi = {
  heldCount(): number;
  heldList(): readonly Held[];
  create(method: Held["method"], fields: object, purpose: Held["purpose"], opts: CreateOptions): { id: string; promise: Promise<object | undefined> };
  answer(id: string, value: AnswerValue): { outcome: "answered" | "cancelled" };
  resolveByHost(id: string, outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void;
  resolveAll(outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void;
};

export function heldCount(): number {
  throw new Error("not implemented");
}

export function heldList(): readonly Held[] {
  throw new Error("not implemented");
}

export function create(method: Held["method"], fields: object, purpose: Held["purpose"], opts: CreateOptions): { id: string; promise: Promise<object | undefined> } {
  throw new Error("not implemented");
}

export function answer(id: string, value: AnswerValue): { outcome: "answered" | "cancelled" } {
  throw new Error("not implemented");
}

export function resolveByHost(id: string, outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void {
  throw new Error("not implemented");
}

export function resolveAll(outcome: Extract<Outcome, "timeout" | "superseded" | "aborted">): void {
  throw new Error("not implemented");
}