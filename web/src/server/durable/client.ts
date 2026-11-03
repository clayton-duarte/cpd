import type { DialogOutcome, ImageMime } from "../../protocol/domain.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";

export interface SocketLike {
  write(line: string): void;
  end(): void;
  onData(handler: (chunk: string) => void): Disposable;
  onClose(handler: (error: Error | null) => void): Disposable;
}
export interface SocketFactory {
  connect(path: string): Promise<SocketLike>;
}
export const PROTOCOL = 1;
export const CLIENT_NAME = "cpd-web";

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

export function parseHostFrame(_line: string): HostFrame {
  throw new Error("not implemented");
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
  abort(): Promise<void>;
  abortSubmission(submissionId: number): Promise<{ outcome: unknown }>;
  respond(id: string, answer: { value: string } | { confirmed: boolean } | { answer: object } | { cancelled: true }): Promise<void>;
  detach(): Promise<void>;
  onFrame(handler: (frame: HostFrame) => void): Disposable;
  onClose(handler: (error: Error | null) => void): Disposable;
  close(): void;
}
export function connectHost(_socketPath: string, _deps: { factory: SocketFactory; scheduler: Scheduler }): Promise<HostClient> {
  return Promise.reject(new Error("not implemented"));
}
