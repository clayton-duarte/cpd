import { type HostState, type ToClient, type ToHost } from "./protocol.ts";
import { type Frame, type Ring } from "./ring.ts";

export type Connection = { write(line: string): void; buffered(): number; close(): void };
export type Timers = { after(ms: number, fn: () => void): () => void };

export type ServerDeps = {
  session: string;
  role: "lead" | "manager";
  cwd: string;
  hostVersion: string;
  piDurable: string;
  epoch: string;
  ring: Ring;
  submit(input: { text: string; whenBusy: "followUp" | "steer" | "reject"; requestId?: string }): Promise<{ submissionId: number; status: string }>;
  abort(opts: { background: boolean }): Promise<void>;
  abortSubmission(id: number): Promise<"aborted" | "already_placed" | "settled" | "not_found">;
  hostState(): HostState;
  timers: Timers;
  clientId(): string;
};

export type Server = {
  connect(conn: Connection): { receive(line: string): void; closed(): void };
  frame(f: Frame): void;
  stateChanged(): void;
  shutdown(reason: "shutdown" | "restart" | "storage_failed"): void;
  clients(): number;
};

export type { ToClient, ToHost };

export function createServer(_deps: ServerDeps): Server {
  throw new Error("not implemented");
}
