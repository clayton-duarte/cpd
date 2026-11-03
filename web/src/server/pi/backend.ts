import type {
  Capabilities,
  ConfigureChange,
  Cursor,
  DialogResponse,
  SessionRef,
  WhenBusy,
  WireImage,
} from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import type { FileBoundary } from "./window.ts";
import type { WatchBoundary } from "./watch.ts";

// Local copies of CONTRACT 6.1 and 6.3 (the base lacks runtime.ts session exports and cockpit/cli.ts);
// to be replaced by imports from runtime.ts and cockpit/cli.ts.
export interface CpdResult {
  readonly stdout: string;
  readonly stderr: string;
  readonly exitCode: number;
}
export interface CpdExec {
  run(args: readonly string[]): Promise<CpdResult>;
}
export interface Cpd {
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>>;
}
export interface ResolvedSession {
  readonly ref: SessionRef;
  readonly file?: string;
  readonly socketPath?: string;
  readonly tierOptions: readonly string[];
}
export type SessionEmit = (message: SessionServerMessage) => void;
export interface GitBoundary {
  branch(cwd: string): Promise<string | null>;
}
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
export type { Disposable };

export function createPiBackend(_deps: {
  cpd: Cpd;
  fs: FileBoundary;
  watch: WatchBoundary;
  git: GitBoundary;
  windowSize?: number;
}): SessionBackend {
  throw new Error("not implemented");
}
