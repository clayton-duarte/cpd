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

export type ErrorCode =
  | "protocol_mismatch" | "hello_timeout" | "bad_frame" | "frame_too_large"
  | "unknown_type" | "bad_args" | "not_attached" | "busy" | "unsupported"
  | "storage_failed" | "shutting_down" | "internal"
  | "hello_required";

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
  | { type: "submit"; req: string; text: string; when_busy: "followUp" | "steer" | "reject"; request_id?: string }
  | { type: "abort"; req: string; background: boolean }
  | { type: "abort_submission"; req: string; submission_id: number }
  | { type: "ping"; req?: string }
  | { type: "detach"; req: string };

export type HostState = {
  state: "idle" | "working" | "waiting" | "blocked";
  name?: string;
  agent?: unknown;
};

export type ToClient =
  | { type: "welcome"; protocol: number; host_version: string; pi_durable: string; session: string; role: "lead" | "manager"; epoch: string; client_id: string; cwd: string; limits: { client_line: number } }
  | { type: "attached"; req: string; mode: "snapshot" | "resume"; epoch: string; from_seq: number; head_seq: number; client_id: string }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "result"; req: string; data: object }
  | { type: "error"; req?: string; code: ErrorCode; message: string; details?: object }
  | { type: "host_state"; state: HostState["state"]; name?: string; clients: number; held_ui: number; agent?: unknown }
  | { type: "pong"; req?: string }
  | { type: "bye"; reason: "shutdown" | "restart" | "storage_failed" | "slow_client" | "detached"; reconnect: boolean };

export function parseLine(
  _line: string,
):
  | { ok: true; msg: ToHost }
  | { ok: false; error: { code: "bad_frame" | "unknown_type" | "bad_args"; message: string; req?: string } } {
  throw new Error("not implemented");
}

export function encode(_msg: ToClient): string {
  throw new Error("not implemented");
}

export function negotiate(_hello: Extract<ToHost, { type: "hello" }>): number | undefined {
  throw new Error("not implemented");
}
