import { toWireError, WebError, type Capabilities, type ConfigureChange, type Cursor, type DialogResponse, type Runner, type SessionRef, type WhenBusy, type WireImage } from "../../protocol/domain.ts";
import { parseClientMessage } from "../../protocol/wire.ts";
import type { ActionOutput, CockpitAction, LogRow, SettingsChanges, SettingsModel } from "../../protocol/cockpit.ts";
import type { ClientMessage, ServerMessage, SessionServerMessage } from "../../protocol/wire.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import type { CockpitSnapshot, CockpitUpdate } from "../cockpit/events.ts";
import type { CompletionTracker } from "../completion-service.ts";
import type { Registry, ResolvedSession } from "./registry.ts";

export interface CockpitService {
  snapshot(): CockpitSnapshot;
  selectPlan(planId: string | null): Promise<void>;
  subscribe(listener: (u: CockpitUpdate) => void): Disposable;
  runAction(action: CockpitAction): Promise<ActionOutput>;
  readLogs?(): Promise<LogRow[]>;
  readSettings?(): Promise<SettingsModel>;
  saveSettings?(changes: SettingsChanges): Promise<void>;
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
  completion?: { track(ref: SessionRef, send: (m: SessionServerMessage) => void): CompletionTracker };
}
export interface HubConnection { handle(raw: unknown): Promise<void>; close(): Promise<void> }

const unsupported = <A extends unknown[], R>(f: ((...a: A) => Promise<R>) | undefined): ((...a: A) => Promise<R>) => {
  if (f === undefined) throw new WebError("internal", "not available on this server");
  return f;
};

// ---- the connection ----
export function createHubConnection(deps: HubDeps): HubConnection {
  let closed = false;
  let subscription: Disposable | null = null;
  let generation = 0;
  let attached: { sessionId: string; handle: SessionHandle; tracker: CompletionTracker | null } | null = null;

  const send = (m: ServerMessage): void => {
    if (!closed) deps.send(m);
  };
  const sessionError = (sessionId: string | null, e: unknown, clientSubmissionId?: string): void =>
    send({ type: "session.error", sessionId, ...(clientSubmissionId === undefined ? {} : { clientSubmissionId }), error: toWireError(e) });

  const detach = async (): Promise<void> => {
    generation += 1; // in-flight attaches and old emits are now stale
    const old = attached;
    attached = null;
    old?.tracker?.dispose();
    if (old !== null) await old.handle.dispose();
  };

  const subscribe = (): void => {
    const snap = deps.cockpit.snapshot();
    if (snap.tree === null || snap.inbox === null) {
      send({ type: "cockpit.error", error: toWireError(new WebError("internal", "cockpit not ready")) });
      return;
    }
    send({ type: "cockpit.snapshot", tree: snap.tree, plan: snap.plan, inbox: snap.inbox, ...(snap.status === undefined ? {} : { status: snap.status }), ...(snap.costs === undefined ? {} : { costs: snap.costs }) });
    subscription ??= deps.cockpit.subscribe((u) => {
      if (u.kind === "error") send({ type: "cockpit.error", error: u.error });
      else send({ type: "cockpit.patch", pane: u.pane, data: u.data } as ServerMessage);
    });
  };

  const attach = async (sessionId: string): Promise<void> => {
    await detach();
    const mine = generation;
    let tracker: CompletionTracker | null = null;
    try {
      const resolved = deps.registry.resolve(sessionId);
      tracker = deps.completion?.track(resolved.ref, (m) => {
        if (generation === mine && !closed) send(m);
      }) ?? null;
      const tracked = tracker;
      const handle = await deps.backends[resolved.ref.runner].attach(resolved, (m) => {
        if (generation !== mine) return;
        tracked?.observe(m);
        send(m);
      });
      if (generation !== mine || closed) {
        tracker?.dispose();
        await handle.dispose();
        return;
      }
      attached = { sessionId, handle, tracker };
    } catch (e) {
      tracker?.dispose();
      if (generation === mine) sessionError(sessionId, e);
    }
  };

  const onSession = async (sessionId: string, clientSubmissionId: string | undefined, run: (h: SessionHandle) => Promise<void>): Promise<void> => {
    if (attached === null || attached.sessionId !== sessionId) {
      sessionError(sessionId, new WebError("not_attached", `session ${sessionId} is not attached`, { sessionId }));
      return;
    }
    try {
      await run(attached.handle);
    } catch (e) {
      sessionError(sessionId, e, clientSubmissionId);
    }
  };

  const isQuit = (text: string): boolean => text.trim() === "/q" || text.trim() === "/quit";
  // /q and /quit archive the session through the cockpit action (pro-d406); nothing reaches the model
  const quit = async (sessionId: string): Promise<void> => {
    const role = deps.registry.resolve(sessionId).ref.role;
    if (role === "manager") throw new WebError("internal", "Manager sessions are archived with their Context");
    const plan = (deps.cockpit.snapshot().tree?.contexts ?? []).flatMap((c) => c.plans).find((p) => p.leadSessionId === sessionId);
    try {
      await deps.cockpit.runAction(plan === undefined ? { kind: "session.archive", sessionId } : { kind: "plan.archive", planId: plan.id });
    } catch (e) {
      if (e instanceof WebError && e.details?.["cpdCode"] === "archive_blocked") throw new WebError(e.code, `archive refused: ${e.message}`, e.details);
      throw e;
    }
  };

  const dispatch = async (m: ClientMessage): Promise<void> => {
    switch (m.type) {
      case "ping":
        send({ type: "pong" });
        return;
      case "cockpit.subscribe": return subscribe();
      case "cockpit.selectPlan":
        try {
          await deps.cockpit.selectPlan(m.planId);
        } catch (e) {
          send({ type: "cockpit.error", error: toWireError(e) });
        }
        return;
      case "cockpit.action":
        try {
          send({ type: "action.result", actionId: m.actionId, ok: true, output: await deps.cockpit.runAction(m.action) });
        } catch (e) {
          send({ type: "action.result", actionId: m.actionId, ok: false, error: toWireError(e) });
        }
        return;
      case "logs.read":
        try {
          send({ type: "logs.result", rows: await unsupported(deps.cockpit.readLogs)() });
        } catch (e) {
          send({ type: "cockpit.error", error: toWireError(e) });
        }
        return;
      case "settings.read":
        try {
          send({ type: "settings.result", settings: await unsupported(deps.cockpit.readSettings)() });
        } catch (e) {
          send({ type: "cockpit.error", error: toWireError(e) });
        }
        return;
      case "settings.save":
        try {
          await unsupported(deps.cockpit.saveSettings)(m.changes);
          send({ type: "settings.saved", ok: true });
        } catch (e) {
          send({ type: "settings.saved", ok: false, error: toWireError(e) });
        }
        return;
      case "session.attach": return attach(m.sessionId);
      case "session.detach": return detach();
      case "session.loadEarlier": return onSession(m.sessionId, undefined, (h) => h.loadEarlier(m.before));
      case "session.submit":
        return onSession(m.sessionId, m.clientSubmissionId, (h) => isQuit(m.text) ? quit(m.sessionId) : h.submit({ clientSubmissionId: m.clientSubmissionId, text: m.text, images: m.images, whenBusy: m.whenBusy }));
      case "session.abort": return onSession(m.sessionId, undefined, (h) => h.abort());
      case "session.abortSubmission": return onSession(m.sessionId, undefined, (h) => h.abortSubmission(m.clientSubmissionId));
      case "session.compact": return onSession(m.sessionId, undefined, (h) => h.compact(m.instructions));
      case "session.configure": return onSession(m.sessionId, undefined, (h) => h.configure(m.change));
      case "session.listModels": return onSession(m.sessionId, undefined, (h) => h.listModels());
      case "dialog.respond": return onSession(m.sessionId, undefined, (h) => h.respond(m.dialogId, m.response));
    }
  };

  return {
    async handle(raw) {
      if (closed) return;
      let message: ClientMessage;
      try {
        message = parseClientMessage(raw);
      } catch (e) {
        sessionError(null, e);
        return;
      }
      await dispatch(message);
    },
    async close() {
      if (closed) return;
      await detach();
      closed = true;
      subscription?.dispose();
      subscription = null;
    },
  };
}
