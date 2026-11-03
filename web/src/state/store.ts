import type {
  Capabilities, Dialog, FooterStats, ModelChoice, QueuedSubmission, SessionRef, WhenBusy, WireError,
} from "../protocol/domain.ts";
import type { ContextTreeModel, InboxModel, PlanViewModel } from "../protocol/cockpit.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { emptyTranscript, type TranscriptState } from "./transcript.ts";

export type ConnectionStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface PendingSubmission {
  readonly clientSubmissionId: string;
  readonly text: string;
  readonly imageCount: number;
  readonly whenBusy: WhenBusy;
  readonly state: "sending" | "failed";
  readonly error?: WireError;
}
export interface SessionView {
  readonly id: string;
  readonly attached: boolean;
  readonly ref: SessionRef | null;
  readonly title: string | null;
  readonly footer: FooterStats | null;
  readonly capabilities: Capabilities | null;
  readonly transcript: TranscriptState;
  readonly busy: boolean;
  readonly queued: readonly QueuedSubmission[];
  readonly pending: readonly PendingSubmission[];
  readonly dialog: Dialog | null;
  readonly models: readonly ModelChoice[] | null;
  readonly error: WireError | null;
}
export interface AppState {
  readonly connection: ConnectionStatus;
  readonly retryAttempt: number;
  readonly cockpit: {
    readonly tree: ContextTreeModel | null;
    readonly plan: PlanViewModel | null;
    readonly inbox: InboxModel | null;
    readonly selectedPlanId: string | null;
    readonly error: WireError | null;
  };
  readonly selectedSessionId: string | null;
  readonly session: SessionView | null;
  readonly lastError: WireError | null;
}

export type StoreAction =
  | { type: "connection"; status: ConnectionStatus; retryAttempt?: number }
  | { type: "server"; message: ServerMessage }
  | { type: "selectSession"; sessionId: string | null }
  | { type: "selectPlan"; planId: string | null }
  | { type: "submitPending"; submission: Omit<PendingSubmission, "state"> }
  | { type: "dismissPending"; clientSubmissionId: string };

export const initialState: AppState = {
  connection: "connecting",
  retryAttempt: 0,
  cockpit: { tree: null, plan: null, inbox: null, selectedPlanId: null, error: null },
  selectedSessionId: null,
  session: null,
  lastError: null,
};
void emptyTranscript;

export function reduce(_state: AppState, _action: StoreAction): AppState {
  throw new Error("not implemented");
}

export interface Store {
  getState(): AppState;
  subscribe(listener: () => void): () => void;
  dispatch(action: StoreAction): void;
}
export function createStore(_initial?: AppState): Store {
  throw new Error("not implemented");
}
export function useStore<T>(_store: Store, _select: (s: AppState) => T): T {
  throw new Error("not implemented");
}
