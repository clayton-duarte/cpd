import { IMAGE_MAX_COUNT, IMAGE_MAX_TOTAL_BASE64, IMAGE_MIME_TYPES, toWireError, WebError, type Capabilities, type ConfigureChange, type Cursor, type DialogResponse, type ImageMime, type Runner, type WhenBusy, type WireImage } from "../../protocol/domain.ts";
import type { ActionOutput, CockpitAction, SnoozeDuration } from "../../protocol/cockpit.ts";
import type { ClientMessage, CockpitPaneData, ServerMessage, SessionServerMessage } from "../../protocol/wire.ts";
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

// ---- validation: the one parse of client data (CONTRACT 4.3, 5.5) ----
type Obj = Readonly<Record<string, unknown>>;
const bad = (message: string, field?: string): WebError => new WebError("invalid_message", message, field === undefined ? undefined : { field });
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (o: Obj, field: string): string => {
  const v = o[field];
  if (typeof v !== "string") throw bad(`${field} must be a string`, field);
  return v;
};
const nonEmpty = (o: Obj, field: string): string => {
  const v = str(o, field);
  if (v === "") throw bad(`${field} must not be empty`, field);
  return v;
};
const objOf = (o: Obj, field: string): Obj => {
  const v = o[field];
  if (!isObj(v)) throw bad(`${field} must be an object`, field);
  return v;
};

const SNOOZE: readonly string[] = ["15m", "1h", "tomorrow"];
function parseAction(raw: unknown): CockpitAction {
  if (!isObj(raw) || typeof raw["kind"] !== "string") throw new WebError("unknown_action", "action has no kind");
  const itemId = (): number => {
    const v = raw["itemId"];
    if (typeof v !== "number" || !Number.isInteger(v) || v < 0) throw bad("itemId must be an integer >= 0", "itemId");
    return v;
  };
  switch (raw["kind"]) {
    case "inbox.snooze": {
      const duration = str(raw, "duration");
      if (!SNOOZE.includes(duration)) throw bad("duration is not one of 15m, 1h, tomorrow", "duration");
      return { kind: "inbox.snooze", itemId: itemId(), duration: duration as SnoozeDuration };
    }
    case "inbox.dismiss": return { kind: "inbox.dismiss", itemId: itemId() };
    case "inbox.claim": return { kind: "inbox.claim", itemId: itemId() };
    case "inbox.approve": return { kind: "inbox.approve", itemId: itemId() };
    case "pr.post": return { kind: "pr.post", planId: nonEmpty(raw, "planId"), workflow: nonEmpty(raw, "workflow") };
    case "session.retier": return { kind: "session.retier", sessionId: nonEmpty(raw, "sessionId"), tier: nonEmpty(raw, "tier") };
    case "plan.reject": return { kind: "plan.reject", planId: nonEmpty(raw, "planId"), review: nonEmpty(raw, "review") };
    default: throw new WebError("unknown_action", `unknown action kind ${raw["kind"]}`, { kind: raw["kind"] });
  }
}

function parseImages(o: Obj): WireImage[] {
  const v = o["images"];
  if (!Array.isArray(v)) throw bad("images must be an array", "images");
  if (v.length > IMAGE_MAX_COUNT) throw bad("too many images", "images");
  let total = 0;
  const images = v.map((img: unknown): WireImage => {
    if (!isObj(img)) throw bad("image must be an object", "images");
    const mimeType = str(img, "mimeType");
    if (!(IMAGE_MIME_TYPES as readonly string[]).includes(mimeType)) throw bad("unsupported image type", "images");
    const dataBase64 = str(img, "dataBase64");
    total += dataBase64.length;
    return { mimeType: mimeType as ImageMime, dataBase64 };
  });
  if (total > IMAGE_MAX_TOTAL_BASE64) throw bad("images too large", "images");
  return images;
}

function parseChange(o: Obj): ConfigureChange {
  const c = objOf(o, "change");
  switch (c["kind"]) {
    case "model": return { kind: "model", model: nonEmpty(c, "model") };
    case "tier": return { kind: "tier", tier: nonEmpty(c, "tier") };
    case "thinking": {
      const level = c["level"];
      if (level !== null && typeof level !== "string") throw bad("level must be a string or null", "level");
      return { kind: "thinking", level };
    }
    default: throw bad("change.kind must be model, thinking or tier", "change");
  }
}

function parseResponse(o: Obj): DialogResponse {
  const r = objOf(o, "response");
  const method = r["method"];
  const outcome = r["outcome"];
  if (method === "ask_user_question") {
    if (outcome === "cancelled") return { method, outcome };
    if (outcome === "answered") {
      const result = objOf(r, "result");
      if (!Array.isArray(result["answers"]) || typeof result["cancelled"] !== "boolean") throw bad("result is not a questionnaire result", "result");
      return { method, outcome, result: result as unknown as Extract<DialogResponse, { outcome: "answered" }>["result"] };
    }
  } else if (method === "cpd_plan_review") {
    if (outcome === "approve" || outcome === "reject" || outcome === "cancelled") return { method, outcome };
    if (outcome === "changes") {
      const feedback = str(r, "feedback");
      if (feedback.trim() === "") throw bad("feedback must not be blank", "feedback");
      return { method, outcome, feedback };
    }
  }
  throw bad("response method/outcome pair is not valid", "response");
}

export function parseClientMessage(raw: unknown): ClientMessage {
  if (!isObj(raw) || typeof raw["type"] !== "string") throw bad("message must be an object with a string type", "type");
  const o = raw;
  const sessionId = (): string => nonEmpty(o, "sessionId");
  switch (o["type"]) {
    case "cockpit.subscribe": return { type: "cockpit.subscribe" };
    case "cockpit.selectPlan": {
      const planId = o["planId"];
      if (planId !== null && typeof planId !== "string") throw bad("planId must be a string or null", "planId");
      return { type: "cockpit.selectPlan", planId };
    }
    case "cockpit.action": return { type: "cockpit.action", actionId: nonEmpty(o, "actionId"), action: parseAction(o["action"]) };
    case "session.attach": return { type: "session.attach", sessionId: sessionId() };
    case "session.detach": return { type: "session.detach" };
    case "session.loadEarlier": return { type: "session.loadEarlier", sessionId: sessionId(), before: nonEmpty(o, "before") };
    case "session.submit": {
      const whenBusy = str(o, "whenBusy");
      if (whenBusy !== "followUp" && whenBusy !== "steer") throw bad("whenBusy must be followUp or steer", "whenBusy");
      return { type: "session.submit", sessionId: sessionId(), clientSubmissionId: nonEmpty(o, "clientSubmissionId"), text: str(o, "text"), images: parseImages(o), whenBusy };
    }
    case "session.abort": return { type: "session.abort", sessionId: sessionId() };
    case "session.abortSubmission": return { type: "session.abortSubmission", sessionId: sessionId(), clientSubmissionId: nonEmpty(o, "clientSubmissionId") };
    case "session.compact": {
      const id = sessionId();
      if (o["instructions"] === undefined) return { type: "session.compact", sessionId: id };
      return { type: "session.compact", sessionId: id, instructions: str(o, "instructions") };
    }
    case "session.configure": return { type: "session.configure", sessionId: sessionId(), change: parseChange(o) };
    case "session.listModels": return { type: "session.listModels", sessionId: sessionId() };
    case "dialog.respond": return { type: "dialog.respond", sessionId: sessionId(), dialogId: nonEmpty(o, "dialogId"), response: parseResponse(o) };
    default: throw bad(`unknown message type ${o["type"]}`, "type");
  }
}

// ---- the connection ----
export function createHubConnection(deps: HubDeps): HubConnection {
  let closed = false;
  let subscription: Disposable | null = null;
  let generation = 0;
  let attached: { sessionId: string; handle: SessionHandle } | null = null;

  const send = (m: ServerMessage): void => {
    if (!closed) deps.send(m);
  };
  const sessionError = (sessionId: string | null, e: unknown, clientSubmissionId?: string): void =>
    send({ type: "session.error", sessionId, ...(clientSubmissionId === undefined ? {} : { clientSubmissionId }), error: toWireError(e) });

  const detach = async (): Promise<void> => {
    generation += 1; // in-flight attaches and old emits are now stale
    const old = attached;
    attached = null;
    if (old !== null) await old.handle.dispose();
  };

  const subscribe = (): void => {
    const snap = deps.cockpit.snapshot();
    if (snap.tree === null || snap.inbox === null) {
      send({ type: "cockpit.error", error: toWireError(new WebError("internal", "cockpit not ready")) });
      return;
    }
    send({ type: "cockpit.snapshot", tree: snap.tree, plan: snap.plan, inbox: snap.inbox });
    subscription ??= deps.cockpit.subscribe((u) => {
      if (u.kind === "error") send({ type: "cockpit.error", error: u.error });
      else send({ type: "cockpit.patch", pane: u.pane, data: u.data } as ServerMessage);
    });
  };

  const attach = async (sessionId: string): Promise<void> => {
    await detach();
    const mine = generation;
    try {
      const resolved = deps.registry.resolve(sessionId);
      const handle = await deps.backends[resolved.ref.runner].attach(resolved, (m) => {
        if (generation === mine) send(m);
      });
      if (generation !== mine || closed) {
        await handle.dispose();
        return;
      }
      attached = { sessionId, handle };
    } catch (e) {
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

  const dispatch = async (m: ClientMessage): Promise<void> => {
    switch (m.type) {
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
      case "session.attach": return attach(m.sessionId);
      case "session.detach": return detach();
      case "session.loadEarlier": return onSession(m.sessionId, undefined, (h) => h.loadEarlier(m.before));
      case "session.submit":
        return onSession(m.sessionId, m.clientSubmissionId, (h) => h.submit({ clientSubmissionId: m.clientSubmissionId, text: m.text, images: m.images, whenBusy: m.whenBusy }));
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
