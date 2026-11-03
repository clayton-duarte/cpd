import type {
  Capabilities,
  ConfigureChange,
  Cursor,
  DialogResponse,
  SessionRef,
  WhenBusy,
  WireImage,
} from "../../protocol/domain.ts";
import type { Scheduler } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import type { HostClient } from "./client.ts";

// TODO(contract 6.1/6.3): replace these local mirrors with the shared imports
// (runtime.ts backend interfaces, cockpit/cli.ts Cpd) once those modules exist.
export interface Cpd {
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>>;
}
export interface GitBoundary {
  branch(cwd: string): Promise<string | null>;
}
export interface ResolvedSession {
  readonly ref: SessionRef;
  readonly file?: string;
  readonly socketPath?: string;
  readonly tierOptions: readonly string[];
}
export type SessionEmit = (message: SessionServerMessage) => void;
export interface SessionHandle {
  readonly capabilities: Capabilities;
  loadEarlier(before: Cursor): Promise<void>;
  submit(input: { clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy }): Promise<void>;
  abort(): Promise<void>;
  abortSubmission(clientSubmissionId: string): Promise<void>;
  compact(instructions?: string): Promise<void>;
  configure(change: ConfigureChange): Promise<void>;
  listModels(): Promise<void>;
  respond(dialogId: string, response: DialogResponse): Promise<void>;
  dispose(): Promise<void>;
}
export interface SessionBackend {
  attach(session: ResolvedSession, emit: SessionEmit): Promise<SessionHandle>;
}

export function createDurableBackend(_deps: {
  connect: (socketPath: string) => Promise<HostClient>;
  cpd: Cpd;
  git: GitBoundary;
  scheduler: Scheduler;
  windowSize?: number;
}): SessionBackend {
  throw new Error("not implemented");
}
