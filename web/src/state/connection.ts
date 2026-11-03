import type { CockpitAction } from "../protocol/cockpit.ts";
import type { ConfigureChange, DialogResponse, WhenBusy, WireImage } from "../protocol/domain.ts";
import type { Scheduler, Transport } from "../protocol/runtime.ts";
import type { Store } from "./store.ts";

export interface ConnectionOptions {
  transport: Transport;
  scheduler: Scheduler;
  store: Store;
  newId?: () => string;
  backoff?: { baseMs: number; maxMs: number };
}
export interface SubmitInput {
  text: string;
  images: WireImage[];
  whenBusy: WhenBusy;
}
export interface Connection {
  start(): void;
  selectSession(sessionId: string | null): void;
  selectPlan(planId: string | null): void;
  loadEarlier(): void;
  submit(input: SubmitInput): string;
  abort(): void;
  abortSubmission(clientSubmissionId: string): void;
  compact(instructions?: string): void;
  configure(change: ConfigureChange): void;
  listModels(): void;
  respond(dialogId: string, response: DialogResponse): void;
  action(action: CockpitAction): string;
  close(): void;
}
export function createConnection(_options: ConnectionOptions): Connection {
  throw new Error("not implemented");
}
