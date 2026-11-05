import type { CockpitAction, SettingsChanges } from "../protocol/cockpit.ts";
import { WebError, toWireError, type ConfigureChange, type DialogResponse, type WhenBusy, type WireImage } from "../protocol/domain.ts";
import type { Disposable, Scheduler, Transport } from "../protocol/runtime.ts";
import type { ClientMessage, ServerMessage } from "../protocol/wire.ts";
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
  readLogs(): void;
  readSettings(): void;
  saveSettings(changes: SettingsChanges): void;
  close(): void;
}
export const FRAME_MS = 16;
export const PENDING_ECHO_MS = 30000;
const DEFAULT_BACKOFF = { baseMs: 500, maxMs: 10_000 };

// Required fields per server message type: [field, primitive type] (the full shapes are the server's contract).
const SESSION = ["sessionId", "string"] as const;
const REQUIRED: Readonly<Record<ServerMessage["type"], readonly (readonly [string, string])[]>> = {
  "cockpit.snapshot": [["tree", "object"], ["inbox", "object"]],
  "cockpit.patch": [["pane", "string"]],
  "cockpit.error": [["error", "object"]],
  "logs.result": [["rows", "array"]],
  "settings.result": [["settings", "object"]],
  "settings.saved": [["ok", "boolean"]],
  "action.result": [["actionId", "string"], ["ok", "boolean"]],
  "session.attached": [SESSION, ["capabilities", "object"], ["meta", "object"]],
  "session.window": [SESSION, ["mode", "string"], ["window", "object"]],
  "session.append": [SESSION, ["items", "array"]],
  "session.update": [SESSION, ["items", "array"]],
  "session.remove": [SESSION, ["itemId", "string"]],
  "session.delta": [SESSION, ["itemId", "string"], ["blockIndex", "number"], ["blockType", "string"], ["text", "string"]],
  "session.status": [SESSION, ["busy", "boolean"], ["queued", "array"]],
  "session.submitAck": [SESSION, ["clientSubmissionId", "string"], ["queued", "boolean"]],
  "session.footer": [SESSION, ["footer", "object"]],
  "session.title": [SESSION],
  "session.models": [SESSION, ["models", "array"]],
  "session.dialog": [SESSION, ["dialog", "object"]],
  "session.dialogResolved": [SESSION, ["dialogId", "string"], ["outcome", "string"]],
  "session.error": [["error", "object"]],
};

const kindOf = (v: unknown): string => (Array.isArray(v) ? "array" : v === null ? "null" : typeof v);

/** Parses an untrusted server message once; throws WebError("protocol_error"). */
function parseServerMessage(raw: unknown): ServerMessage {
  if (kindOf(raw) !== "object") throw new WebError("protocol_error", "server message is not an object");
  const m = raw as Record<string, unknown>;
  const type = m["type"];
  if (typeof type !== "string" || !Object.hasOwn(REQUIRED, type)) {
    throw new WebError("protocol_error", `unknown server message type ${String(type)}`, { field: "type" });
  }
  for (const [field, kind] of REQUIRED[type as ServerMessage["type"]]) {
    if (kindOf(m[field]) !== kind) {
      throw new WebError("protocol_error", `${type}: field ${field} must be ${kind}`, { field });
    }
  }
  if (type.startsWith("session.") && type !== "session.error" && m["sessionId"] === "") {
    throw new WebError("protocol_error", `${type}: empty sessionId`, { field: "sessionId" });
  }
  if (type === "session.error" && m["sessionId"] !== null && typeof m["sessionId"] !== "string") {
    throw new WebError("protocol_error", "session.error: sessionId must be string or null", { field: "sessionId" });
  }
  return m as unknown as ServerMessage;
}

export function createConnection(options: ConnectionOptions): Connection {
  const { transport, scheduler, store } = options;
  const backoff = options.backoff ?? DEFAULT_BACKOFF;
  let counter = 0;
  const newId = options.newId ?? ((): string => `c${++counter}`);
  let attempt = 0;
  let closed = false;
  let timer: Disposable | null = null;
  let buffer: ServerMessage[] = [];
  let frame: Disposable | null = null;
  const echoTimers = new Map<string, Disposable>();

  const dropBuffer = (): void => {
    frame?.dispose();
    frame = null;
    buffer = [];
  };
  const dropEchoTimers = (): void => {
    for (const t of echoTimers.values()) t.dispose();
    echoTimers.clear();
  };
  const flush = (): void => {
    frame?.dispose();
    frame = null;
    if (buffer.length === 0) return;
    const messages = buffer;
    buffer = [];
    store.dispatch({ type: "serverBatch", messages });
  };
  const enqueue = (message: ServerMessage): void => {
    const last = buffer[buffer.length - 1];
    if (
      message.type === "session.delta" && last?.type === "session.delta" && last.sessionId === message.sessionId &&
      last.itemId === message.itemId && last.blockIndex === message.blockIndex && last.blockType === message.blockType
    ) {
      buffer[buffer.length - 1] = { ...last, text: last.text + message.text };
    } else {
      buffer.push(message);
    }
    frame ??= scheduler.after(FRAME_MS, () => {
      frame = null;
      flush();
    });
  };

  const send = (message: ClientMessage): void => transport.send(message);
  const selected = (): string => {
    const id = store.getState().selectedSessionId;
    if (id === null) throw new WebError("not_attached", "no session is selected");
    return id;
  };

  const objectOf = (v: unknown): Record<string, unknown> | null => (typeof v === "object" && v !== null ? (v as Record<string, unknown>) : null);
  /** A create action's reply names the new Plan and its Session: select and attach them. */
  function selectCreated(output: unknown): void {
    const out = objectOf(output);
    if (out === null) return;
    const planId = objectOf(out["plan"])?.["id"];
    if (typeof planId === "string") {
      store.dispatch({ type: "selectPlan", planId });
      send({ type: "cockpit.selectPlan", planId });
    }
    const lead = objectOf(out["lead"])?.["session"];
    const forked = objectOf(out["session"])?.["id"];
    const next = typeof lead === "string" ? lead : typeof forked === "string" ? forked : null;
    const before = store.getState().selectedSessionId;
    if (next === null || next === before) return;
    dropBuffer();
    dropEchoTimers();
    store.dispatch({ type: "selectSession", sessionId: next });
    if (store.getState().connection !== "open") return;
    if (before !== null) send({ type: "session.detach" });
    send({ type: "session.attach", sessionId: next });
  }

  transport.onMessage((raw) => {
    if (closed) return;
    let message: ServerMessage;
    try {
      message = parseServerMessage(raw);
    } catch (e) {
      flush();
      store.dispatch({ type: "server", message: { type: "session.error", sessionId: null, error: toWireError(e) } });
      return;
    }
    if (
      (message.type === "session.append" || message.type === "session.update" || message.type === "session.delta") &&
      message.sessionId === store.getState().selectedSessionId
    ) {
      enqueue(message);
      return;
    }
    flush();
    store.dispatch({ type: "server", message });
    if (message.type === "action.result" && message.ok) selectCreated(message["output"]);
    if (message.type === "session.submitAck" && !message.queued && message.sessionId === store.getState().selectedSessionId) {
      const id = message.clientSubmissionId;
      echoTimers.get(id)?.dispose();
      echoTimers.set(id, scheduler.after(PENDING_ECHO_MS, () => {
        echoTimers.delete(id);
        store.dispatch({ type: "dismissPending", clientSubmissionId: id });
      }));
    }
  });
  transport.onStatus((status) => {
    if (closed) return;
    if (status === "open") {
      attempt = 0;
      store.dispatch({ type: "connection", status: "open", retryAttempt: 0 });
      send({ type: "cockpit.subscribe" });
      const { cockpit, selectedSessionId } = store.getState();
      if (cockpit.selectedPlanId !== null) send({ type: "cockpit.selectPlan", planId: cockpit.selectedPlanId });
      if (selectedSessionId !== null) send({ type: "session.attach", sessionId: selectedSessionId });
      return;
    }
    timer?.dispose();
    store.dispatch({ type: "connection", status: "reconnecting", retryAttempt: attempt });
    const delay = Math.min(backoff.maxMs, backoff.baseMs * 2 ** attempt);
    attempt++;
    timer = scheduler.after(delay, () => {
      timer = null;
      transport.connect();
    });
  });

  return {
    start() {
      store.dispatch({ type: "connection", status: "connecting" });
      transport.connect();
    },
    selectSession(sessionId) {
      const before = store.getState().selectedSessionId;
      if (before === sessionId) return;
      dropBuffer();
      dropEchoTimers();
      store.dispatch({ type: "selectSession", sessionId });
      if (store.getState().connection !== "open") return; // the attach is sent on open
      if (before !== null) send({ type: "session.detach" });
      if (sessionId !== null) send({ type: "session.attach", sessionId });
    },
    selectPlan(planId) {
      store.dispatch({ type: "selectPlan", planId });
      send({ type: "cockpit.selectPlan", planId });
    },
    loadEarlier() {
      const { session } = store.getState();
      if (session === null || !session.transcript.hasEarlier || session.transcript.cursor === null) return;
      send({ type: "session.loadEarlier", sessionId: session.id, before: session.transcript.cursor });
    },
    submit(input) {
      const sessionId = selected();
      const clientSubmissionId = newId();
      store.dispatch({
        type: "submitPending",
        submission: { clientSubmissionId, text: input.text, imageCount: input.images.length, images: input.images, whenBusy: input.whenBusy, at: new Date().toISOString() },
      });
      try {
        send({
          type: "session.submit", sessionId, clientSubmissionId, text: input.text, images: input.images, whenBusy: input.whenBusy,
        });
      } catch (e) {
        store.dispatch({ type: "dismissPending", clientSubmissionId });
        throw e;
      }
      return clientSubmissionId;
    },
    abort() {
      send({ type: "session.abort", sessionId: selected() });
    },
    abortSubmission(clientSubmissionId) {
      send({ type: "session.abortSubmission", sessionId: selected(), clientSubmissionId });
    },
    compact(instructions) {
      const sessionId = selected();
      send(instructions === undefined ? { type: "session.compact", sessionId } : { type: "session.compact", sessionId, instructions });
    },
    configure(change) {
      send({ type: "session.configure", sessionId: selected(), change });
    },
    listModels() {
      send({ type: "session.listModels", sessionId: selected() });
    },
    respond(dialogId, response) {
      send({ type: "dialog.respond", sessionId: selected(), dialogId, response });
    },
    action(action) {
      const actionId = newId();
      store.dispatch({ type: "actionStart", actionId });
      send({ type: "cockpit.action", actionId, action });
      return actionId;
    },
    readLogs() {
      send({ type: "logs.read" });
    },
    readSettings() {
      send({ type: "settings.read" });
    },
    saveSettings(changes) {
      send({ type: "settings.save", changes });
    },
    close() {
      if (closed) return;
      closed = true;
      timer?.dispose();
      timer = null;
      dropBuffer();
      dropEchoTimers();
      store.dispatch({ type: "connection", status: "closed" });
      transport.close();
    },
  };
}
