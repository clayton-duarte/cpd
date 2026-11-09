import { WebError, type DialogOutcome, type ImageMime } from "../../protocol/domain.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";
import { assertSupportedHost } from "./frames.ts";

export interface SocketLike {
  write(line: string): void;
  end(): void;
  onData(handler: (chunk: string) => void): Disposable;
  onClose(handler: (error: Error | null) => void): Disposable;
}
export interface SocketFactory {
  connect(path: string): Promise<SocketLike>;
}
export const PROTOCOL = 2;
export const MIN_PROTOCOL = 1;
export const CLIENT_NAME = "cpd-web";

export type HostFrame =
  | { type: "welcome"; protocol: number; host_version: string; pi_durable: string; session: string; role: string; epoch: string; client_id: string; cwd: string; limits: { client_line: number } }
  | { type: "attached"; req: string; mode: "snapshot" | "view" | "resume"; epoch: string; from_seq: number; head_seq: number; client_id: string }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "view"; epoch: string; seq: number; view: unknown }
  | { type: "ops"; epoch: string; seq: number; ops: unknown[] }
  | { type: "result"; req: string; data: Record<string, unknown> }
  | { type: "error"; req?: string; code: string; message: string; details?: unknown }
  | { type: "host_state"; state: "idle" | "working" | "waiting" | "blocked"; name?: string; clients: number; held_ui: number; agent?: { model?: { provider: string; model_id: string }; thinking_level?: string | null } }
  | { type: "ui_request"; id: string; purpose: string; created_at: number; expires_at?: number | null; tool_call_id?: string; replayed?: boolean; method: string; [field: string]: unknown }
  | { type: "ui_resolved"; id: string; outcome: DialogOutcome; by?: string }
  | { type: "notice"; level: "info" | "warning" | "error"; message: string }
  | { type: "status"; key: string; text: string | null }
  | { type: "pong"; req?: string }
  | { type: "bye"; reason: "shutdown" | "restart" | "storage_failed" | "slow_client" | "detached"; reconnect: boolean };

const FRAME_TYPES: ReadonlySet<string> = new Set([
  "welcome",
  "attached",
  "snapshot",
  "events",
  "view",
  "ops",
  "result",
  "error",
  "host_state",
  "ui_request",
  "ui_resolved",
  "notice",
  "status",
  "pong",
  "bye",
]);

export function parseHostFrame(line: string): HostFrame {
  const fail = (why: string): never => {
    throw new WebError("protocol_error", `host frame: ${why}`, { line: line.slice(0, 200) });
  };
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    return fail("not valid JSON");
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return fail("not an object");
  const type = (value as { type?: unknown }).type;
  if (typeof type !== "string" || !FRAME_TYPES.has(type)) return fail(`unknown type ${String(type)}`);
  return value as HostFrame;
}

export interface HostCursor {
  epoch: string;
  seq: number;
}
export interface HostClient {
  readonly welcome: Extract<HostFrame, { type: "welcome" }>;
  attach(cursor?: HostCursor): Promise<Extract<HostFrame, { type: "attached" }>>;
  submit(input: { text: string; whenBusy: "followUp" | "steer"; requestId?: string; images?: { mimeType: ImageMime; dataBase64: string }[] }): Promise<{ submissionId: number; status: string }>;
  compact(instructions?: string): Promise<{ taskId: number }>;
  configure(change: { model: string } | { thinkingLevel: string | null }): Promise<Record<string, unknown>>;
  listModels(): Promise<{ provider: string; model_id: string; name: string; thinking_levels: string[]; context_window: number }[]>;
  history(before?: string, limit?: number): Promise<{ entries: readonly unknown[]; next?: string }>;
  abort(): Promise<void>;
  abortSubmission(submissionId: number): Promise<{ outcome: unknown }>;
  respond(id: string, answer: { value: string } | { confirmed: boolean } | { answer: object } | { cancelled: true }): Promise<void>;
  detach(): Promise<void>;
  onFrame(handler: (frame: HostFrame) => void): Disposable;
  onClose(handler: (error: Error | null) => void): Disposable;
  close(): void;
}
const HELLO_MS = 5000; // LIMITS.helloMs, protocol.ts:6

type Pending = { resolve: (frame: HostFrame) => void; reject: (error: Error) => void };
type Answer = Parameters<HostClient["respond"]>[1];

export async function connectHost(socketPath: string, deps: { factory: SocketFactory; scheduler: Scheduler; webuiToken?: string }): Promise<HostClient> {
  let socket: SocketLike;
  try {
    socket = await deps.factory.connect(socketPath);
  } catch (e) {
    throw new WebError("host_unreachable", `cannot open ${socketPath}: ${e instanceof Error ? e.message : String(e)}`, { path: socketPath });
  }

  const frameHandlers = new Set<(frame: HostFrame) => void>();
  const closeHandlers = new Set<(error: Error | null) => void>();
  const pending = new Map<string, Pending>();
  let closed = false;
  let buffer = "";
  let nextReq = 0;
  let welcome: Extract<HostFrame, { type: "welcome" }> | null = null;
  let onHandshake: ((outcome: Error | Extract<HostFrame, { type: "welcome" }>) => void) | null = null;

  const subscriptions: Disposable[] = [];
  const teardown = (error: Error): void => {
    closed = true;
    for (const s of subscriptions) s.dispose();
    for (const p of pending.values()) p.reject(error);
    pending.clear();
  };
  const fail = (error: Error): void => {
    if (closed) return;
    teardown(error);
    if (onHandshake !== null) {
      onHandshake(error);
      return;
    }
    for (const h of [...closeHandlers]) h(error);
  };
  const disconnected = (details?: Record<string, unknown>): WebError =>
    new WebError("host_disconnected", "durable host connection closed", details);

  const dispatch = (frame: HostFrame): void => {
    if (welcome === null) {
      if (frame.type === "welcome") {
        onHandshake?.(frame);
      } else if (frame.type === "error") {
        fail(new WebError("host_error", frame.message, { hostCode: frame.code, details: frame.details }));
      }
      return;
    }
    if ((frame.type === "result" || frame.type === "error" || frame.type === "attached") && frame.req !== undefined) {
      const p = pending.get(frame.req);
      if (p !== undefined) {
        pending.delete(frame.req);
        if (frame.type === "error") p.reject(new WebError("host_error", frame.message, { hostCode: frame.code, details: frame.details }));
        else p.resolve(frame);
        return;
      }
    }
    for (const h of [...frameHandlers]) h(frame);
    if (frame.type === "bye") fail(disconnected({ bye: frame.reason }));
  };

  subscriptions.push(
    socket.onData((chunk) => {
      buffer += chunk;
      for (;;) {
        const nl = buffer.indexOf("\n");
        if (nl < 0) return;
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (line === "") continue;
        let frame: HostFrame;
        try {
          frame = parseHostFrame(line);
        } catch (e) {
          fail(e as Error);
          socket.end();
          return;
        }
        if (closed) return;
        dispatch(frame);
      }
    }),
    socket.onClose(() => fail(onHandshake !== null ? new WebError("host_unreachable", "socket closed before welcome", { path: socketPath }) : disconnected())),
  );

  const first = await new Promise<Error | Extract<HostFrame, { type: "welcome" }>>((resolve) => {
    const timer = deps.scheduler.after(HELLO_MS, () =>
      resolve(new WebError("host_unreachable", `no welcome within ${HELLO_MS} ms`, { path: socketPath })),
    );
    onHandshake = (outcome) => {
      timer.dispose();
      resolve(outcome);
    };
    socket.write(`${JSON.stringify({ type: "hello", protocol: PROTOCOL, min_protocol: MIN_PROTOCOL, client: CLIENT_NAME, ...(deps.webuiToken === undefined ? {} : { webui_token: deps.webuiToken }) })}\n`);
  });
  onHandshake = null;
  if (first instanceof Error) {
    if (!closed) {
      teardown(first);
      socket.end();
    }
    throw first;
  }
  try {
    assertSupportedHost(first);
  } catch (e) {
    teardown(e as Error);
    socket.end();
    throw e;
  }
  welcome = first;

  const request = (type: string, fields: Record<string, unknown>): Promise<HostFrame> => {
    if (closed) return Promise.reject(disconnected());
    const req = `r${++nextReq}`;
    return new Promise<HostFrame>((resolve, reject) => {
      pending.set(req, { resolve, reject });
      socket.write(`${JSON.stringify({ type, req, ...fields })}\n`);
    });
  };
  const data = async (type: string, fields: Record<string, unknown>): Promise<Record<string, unknown>> => {
    const frame = await request(type, fields);
    if (frame.type !== "result") throw new WebError("protocol_error", `${type}: expected result, got ${frame.type}`);
    return frame.data;
  };
  const bad = (type: string, field: string): never => {
    throw new WebError("protocol_error", `${type} result: bad ${field}`);
  };

  return {
    welcome,
    async attach(cursor) {
      const frame = await request("attach", { conversation: "root", ...(cursor === undefined ? {} : { cursor }) });
      if (frame.type !== "attached") throw new WebError("protocol_error", `attach: expected attached, got ${frame.type}`);
      return frame;
    },
    async submit(input) {
      const result = await data("submit", {
        text: input.text,
        when_busy: input.whenBusy,
        ...(input.requestId === undefined ? {} : { request_id: input.requestId }),
        ...(input.images === undefined || input.images.length === 0
          ? {}
          : { images: input.images.map((i) => ({ mime_type: i.mimeType, data: i.dataBase64 })) }),
      });
      const { submission_id: id, status } = result;
      if (typeof id !== "number") return bad("submit", "submission_id");
      if (typeof status !== "string") return bad("submit", "status");
      return { submissionId: id, status };
    },
    async compact(instructions) {
      const result = await data("compact", instructions === undefined ? {} : { instructions });
      if (typeof result.task_id !== "number") return bad("compact", "task_id");
      return { taskId: result.task_id };
    },
    configure(change) {
      return data("configure", "model" in change ? { model: change.model } : { thinking_level: change.thinkingLevel });
    },
    async listModels() {
      const { models } = await data("list_models", {});
      if (!Array.isArray(models)) return bad("list_models", "models");
      return models as Awaited<ReturnType<HostClient["listModels"]>>;
    },
    async history(before, limit) {
      const result = await data("history", { ...(before === undefined ? {} : { before }), ...(limit === undefined ? {} : { limit }) });
      const { entries, next } = result;
      if (!Array.isArray(entries)) return bad("history", "entries");
      if (next !== undefined && typeof next !== "string") return bad("history", "next");
      return { entries, ...(next === undefined ? {} : { next }) };
    },
    async abort() {
      await data("abort", { background: false });
    },
    async abortSubmission(submissionId) {
      const result = await data("abort_submission", { submission_id: submissionId });
      return { outcome: result.outcome };
    },
    async respond(id: string, answer: Answer) {
      await data("ui_response", { id, ...answer });
    },
    async detach() {
      await data("detach", {});
    },
    onFrame(handler) {
      frameHandlers.add(handler);
      return { dispose: () => void frameHandlers.delete(handler) };
    },
    onClose(handler) {
      closeHandlers.add(handler);
      return { dispose: () => void closeHandlers.delete(handler) };
    },
    close() {
      if (closed) return;
      teardown(disconnected());
      socket.end();
    },
  };
}
