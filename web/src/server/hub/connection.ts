// STUB (red-test Job): the implementation Job replaces the bodies. Shared types are declared here only until their owning modules exist.
import type { Capabilities, ConfigureChange, Cursor, DialogResponse, Runner, WhenBusy, WireImage } from "../../protocol/domain.ts";
import type { ActionOutput, CockpitAction } from "../../protocol/cockpit.ts";
import type { CockpitPaneData, ServerMessage, SessionServerMessage } from "../../protocol/wire.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { WireError } from "../../protocol/domain.ts";
import type { CockpitSnapshot, Registry, ResolvedSession } from "./registry.ts";

export type CockpitUpdate = ({ kind: "pane" } & CockpitPaneData) | { kind: "error"; error: WireError };
export interface CockpitService {
  snapshot(): CockpitSnapshot;
  selectPlan(planId: string | null): Promise<void>;
  subscribe(listener: (u: CockpitUpdate) => void): Disposable;
  runAction(action: CockpitAction): Promise<ActionOutput>;
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
export interface SessionBackend { attach(session: ResolvedSession, emit: SessionEmit): Promise<SessionHandle> }

export interface HubDeps {
  cockpit: CockpitService;
  registry: Registry;
  backends: Readonly<Record<Runner, SessionBackend>>;
  send: (message: ServerMessage) => void;
}
export interface HubConnection { handle(raw: unknown): Promise<void>; close(): Promise<void> }
export function createHubConnection(_deps: HubDeps): HubConnection {
  throw new Error("not implemented");
}
