import { useSyncExternalStore } from "react";
import type {
  Capabilities, Dialog, FooterStats, ModelChoice, QueuedSubmission, SessionRef, WhenBusy, WireError, WireImage,
} from "../protocol/domain.ts";
import type { ContextTreeModel, CostModel, InboxModel, LogRow, PlanViewModel, SettingsModel, StatusModel } from "../protocol/cockpit.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import type { TranscriptItem } from "../protocol/domain.ts";
import { toWireError } from "../protocol/domain.ts";
import {
  appendItems, applyDelta, emptyTranscript, prependEarlier, setTail, updateItems, type TranscriptState,
} from "./transcript.ts";

export type ConnectionStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface PendingSubmission {
  readonly clientSubmissionId: string;
  readonly text: string;
  readonly imageCount: number;
  readonly images?: readonly WireImage[];
  readonly whenBusy: WhenBusy;
  readonly state: "sending" | "sent" | "failed";
  readonly at?: string;
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
    readonly status: StatusModel | null;
    readonly costs: CostModel | null;
    /** The last `logs.result`; null until requested. */
    readonly logs: readonly LogRow[] | null;
    /** The last `settings.result`; null until requested. */
    readonly settings: SettingsModel | null;
    /** The last `settings.saved`; null until a save was answered. */
    readonly saved: { readonly ok: true } | { readonly ok: false; readonly error: WireError } | null;
  };
  readonly selectedSessionId: string | null;
  readonly session: SessionView | null;
  readonly lastError: WireError | null;
  readonly pendingActionIds: readonly string[];
  readonly completedActionIds: readonly string[];
}

export type StoreAction =
  | { type: "connection"; status: ConnectionStatus; retryAttempt?: number }
  | { type: "server"; message: ServerMessage }
  | { type: "selectSession"; sessionId: string | null }
  | { type: "selectPlan"; planId: string | null }
  | { type: "submitPending"; submission: Omit<PendingSubmission, "state"> }
  | { type: "dismissPending"; clientSubmissionId: string }
  | { type: "actionStart"; actionId: string }
  | { type: "dismissError" }
  | { type: "serverBatch"; messages: ServerMessage[] };

export const initialState: AppState = {
  connection: "connecting",
  retryAttempt: 0,
  cockpit: { tree: null, plan: null, inbox: null, selectedPlanId: null, error: null, status: null, costs: null, logs: null, settings: null, saved: null },
  selectedSessionId: null,
  session: null,
  lastError: null,
  pendingActionIds: [],
  completedActionIds: [],
};
const emptySession = (id: string): SessionView => ({
  id, attached: false, ref: null, title: null, footer: null, capabilities: null, transcript: emptyTranscript,
  busy: false, queued: [], pending: [], dialog: null, models: null, error: null,
});

function normalizeUserText(text: string): string {
  return text
    .replace(/^Message from [^\n:]+:\s*/, "")
    .replace(/(?:\n+\[Attached image: [^\]]+\])+\s*$/, "")
    .trim();
}

/** Removes the oldest matching "sending" or "sent" row per new user item (id not in `before`). */
function echoed(pending: readonly PendingSubmission[], before: TranscriptState, incoming: readonly TranscriptItem[]): readonly PendingSubmission[] {
  let rows = pending;
  const known = new Set(before.items.map((i) => i.id));
  for (const item of incoming) {
    if (item.kind !== "user" || known.has(item.id)) continue;
    const norm = normalizeUserText(item.text);
    const at = rows.findIndex(
      (p) => (p.state === "sent" || p.state === "sending") && normalizeUserText(p.text) === norm,
    );
    if (at >= 0) rows = rows.filter((_, k) => k !== at);
  }
  return rows;
}

function reduceSession(state: AppState, session: SessionView, message: ServerMessage): AppState {
  const set = (patch: Partial<SessionView>): AppState => ({ ...state, session: { ...session, ...patch } });
  switch (message.type) {
    case "session.attached":
      return set({ attached: true, capabilities: message.capabilities, title: message.title, ref: message.meta.ref, footer: message.meta.footer });
    case "session.window":
      if (message.mode === "tail") {
        return set({
          transcript: setTail(message.window, session.transcript),
          pending: session.pending.filter((p) => p.state !== "sent"),
        });
      }
      return set({
        transcript: prependEarlier(session.transcript, message.window),
        pending: echoed(session.pending, session.transcript, message.window.items),
      });
    case "session.append":
      return set({
        transcript: appendItems(session.transcript, message.items),
        pending: echoed(session.pending, session.transcript, message.items),
      });
    case "session.update":
      return set({
        transcript: updateItems(session.transcript, message.items),
        pending: echoed(session.pending, session.transcript, message.items),
      });
    case "session.delta":
      return set({ transcript: applyDelta(session.transcript, message) });
    case "session.status":
      return set({ busy: message.busy, queued: message.queued });
    case "session.footer":
      return set({ footer: message.footer });
    case "session.title":
      return set({ title: message.title });
    case "session.models":
      return set({ models: message.models });
    case "session.dialog":
      return set({ dialog: message.dialog });
    case "session.dialogResolved":
      return session.dialog !== null && session.dialog.id === message.dialogId ? set({ dialog: null }) : state;
    case "session.submitAck": {
      if (message.queued) {
        return set({
          pending: session.pending.filter((p) => p.clientSubmissionId !== message.clientSubmissionId),
        });
      }
      const sub = session.pending.find((p) => p.clientSubmissionId === message.clientSubmissionId);
      const alreadyInTranscript =
        sub !== undefined &&
        session.transcript.items.some(
          (i) => i.kind === "user" && normalizeUserText(i.text) === normalizeUserText(sub.text),
        );
      return set({
        pending: alreadyInTranscript
          ? session.pending.filter((p) => p.clientSubmissionId !== message.clientSubmissionId)
          : session.pending.map((p) =>
              p.clientSubmissionId === message.clientSubmissionId ? { ...p, state: "sent" as const } : p,
            ),
      });
    }
    case "session.error": {
      const id = message.clientSubmissionId;
      if (id === undefined) return set({ error: message.error });
      return set({
        pending: session.pending.map((p) =>
          p.clientSubmissionId === id ? { ...p, state: "failed" as const, error: message.error } : p),
      });
    }
    default:
      return state;
  }
}

function reduceServer(state: AppState, message: ServerMessage): AppState {
  switch (message.type) {
    case "cockpit.snapshot":
      return { ...state, cockpit: { ...state.cockpit, tree: message.tree, plan: message.plan, inbox: message.inbox, status: message.status ?? state.cockpit.status, costs: message.costs ?? state.cockpit.costs, error: null } };
    case "logs.result":
      return { ...state, cockpit: { ...state.cockpit, logs: message.rows } };
    case "settings.result":
      return { ...state, cockpit: { ...state.cockpit, settings: message.settings } };
    case "settings.saved":
      return { ...state, cockpit: { ...state.cockpit, saved: message.ok ? { ok: true } : { ok: false, error: message.error } } };
    case "cockpit.patch":
      return { ...state, cockpit: { ...state.cockpit, [message.pane]: message.data } };
    case "cockpit.error":
      return { ...state, cockpit: { ...state.cockpit, error: message.error } };
    case "action.result": {
      const pendingActionIds = state.pendingActionIds.filter((id) => id !== message.actionId);
      if (message.ok) {
        return {
          ...state,
          pendingActionIds,
          completedActionIds: state.completedActionIds.includes(message.actionId)
            ? state.completedActionIds
            : [...state.completedActionIds, message.actionId],
        };
      }
      return {
        ...state,
        pendingActionIds,
        lastError: message.error,
      };
    }
    default: {
      if (message.sessionId === null) {
        return message.type === "session.error" ? { ...state, lastError: message.error } : state;
      }
      if (state.session === null || message.sessionId !== state.selectedSessionId) return state;
      return reduceSession(state, state.session, message);
    }
  }
}

export function reduce(state: AppState, action: StoreAction): AppState {
  switch (action.type) {
    case "connection":
      return { ...state, connection: action.status, retryAttempt: action.retryAttempt ?? state.retryAttempt };
    case "server":
      try {
        return reduceServer(state, action.message);
      } catch (e) {
        return { ...state, lastError: toWireError(e) };
      }
    case "selectSession":
      if (action.sessionId === state.selectedSessionId) return state;
      return {
        ...state,
        selectedSessionId: action.sessionId,
        session: action.sessionId === null ? null : emptySession(action.sessionId),
      };
    case "selectPlan":
      return { ...state, cockpit: { ...state.cockpit, selectedPlanId: action.planId } };
    case "submitPending":
      if (state.session === null) return state;
      return {
        ...state,
        session: {
          ...state.session,
          pending: [
            ...state.session.pending,
            {
              ...action.submission,
              at: action.submission.at ?? new Date().toISOString(),
              state: "sending",
            },
          ],
        },
      };
    case "serverBatch": {
      let next = state;
      for (const message of action.messages) next = reduce(next, { type: "server", message });
      return next;
    }
    case "dismissPending":
      if (state.session === null) return state;
      return {
        ...state,
        session: { ...state.session, pending: state.session.pending.filter((p) => p.clientSubmissionId !== action.clientSubmissionId) },
      };
    case "actionStart":
      return {
        ...state,
        pendingActionIds: state.pendingActionIds.includes(action.actionId)
          ? state.pendingActionIds
          : [...state.pendingActionIds, action.actionId],
      };
    case "dismissError":
      return {
        ...state,
        lastError: null,
        cockpit: { ...state.cockpit, error: null },
      };
  }
}

export interface Store {
  getState(): AppState;
  subscribe(listener: () => void): () => void;
  dispatch(action: StoreAction): void;
}
export function createStore(initial: AppState = initialState): Store {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    getState: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    dispatch(action) {
      const next = reduce(state, action);
      if (next === state) return;
      state = next;
      for (const l of [...listeners]) l();
    },
  };
}
export function useStore<T>(store: Store, select: (s: AppState) => T): T {
  return useSyncExternalStore(store.subscribe, () => select(store.getState()));
}
