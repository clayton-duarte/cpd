// Browser/server wire types (web/CONTRACT.md section 4). Types, plus the client-message parser (parseClientMessage).
import { IMAGE_MAX_COUNT, IMAGE_MAX_TOTAL_BASE64, IMAGE_MIME_TYPES, WebError, type Capabilities, type Completion, type CompletionScore, type ImageMime, type ConfigureChange, type Cursor, type Dialog, type DialogOutcome, type DialogResponse, type FooterStats, type ModelChoice, type QueuedSubmission, type SessionRef, type TranscriptItem, type WhenBusy, type Window, type WireError, type WireImage } from "./domain.ts";
import { parseCockpitAction, type ActionOutput, type CockpitAction, type ContextTreeModel, type CostModel, type InboxModel, type LogRow, type PlanViewModel, type SettingsChanges, type SettingsModel, type StatusModel, SETTINGS_ROLES, SETTINGS_SWITCHES } from "./cockpit.ts";


export type ClientMessage =
  | { type: "ping" } // the client's wire heartbeat (15.2); the server answers { type: "pong" } at once, with no cockpit or session involvement
  | { type: "cockpit.subscribe" } // idempotent; the server answers with cockpit.snapshot (all panes)
  | { type: "cockpit.selectPlan"; planId: string | null } // choose the Plan shown in the Plan pane; null clears it
  | { type: "cockpit.action"; actionId: string; action: CockpitAction } // actionId: client correlation id, non-empty
  | { type: "session.attach"; sessionId: string } // attaches this connection to that session; an already attached session is detached first
  | { type: "session.detach" } // idempotent
  | { type: "session.loadEarlier"; sessionId: string; before: Cursor }
  | { type: "session.submit"; sessionId: string; clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy }
  | { type: "session.abort"; sessionId: string }
  | { type: "session.abortSubmission"; sessionId: string; clientSubmissionId: string }
  | { type: "session.compact"; sessionId: string; instructions?: string }
  | { type: "session.configure"; sessionId: string; change: ConfigureChange }
  | { type: "session.listModels"; sessionId: string }
  | { type: "logs.read" } // answered with logs.result
  | { type: "settings.read" } // answered with settings.result
  | { type: "settings.save"; changes: SettingsChanges } // answered with settings.saved
  | { type: "dialog.respond"; sessionId: string; dialogId: string; response: DialogResponse };


export type CockpitPane = "tree" | "plan" | "inbox" | "status";
export type CockpitPaneData =
  | { pane: "tree"; data: ContextTreeModel }
  | { pane: "plan"; data: PlanViewModel | null } // null = no Plan selected
  | { pane: "inbox"; data: InboxModel }
  | { pane: "status"; data: StatusModel }
  | { pane: "costs"; data: CostModel };

export interface SessionMeta { ref: SessionRef; footer: FooterStats }

export type ServerMessage =
  | { type: "pong" } // the answer to the client's wire `ping` (15.2); no cockpit or session state
  // cockpit
  | { type: "cockpit.snapshot"; tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel; status?: StatusModel; costs?: CostModel } // all panes; sent on subscribe
  | ({ type: "cockpit.patch" } & CockpitPaneData) // replaces ONE pane (an invalidation refresh); "patch" = per-pane replacement
  | { type: "logs.result"; rows: LogRow[] }
  | { type: "settings.result"; settings: SettingsModel }
  | { type: "settings.saved"; ok: true }
  | { type: "settings.saved"; ok: false; error: WireError }
  | { type: "cockpit.error"; error: WireError } // a refresh failed; the previous snapshot stays valid
  | { type: "action.result"; actionId: string; ok: true; output: ActionOutput }
  | { type: "action.result"; actionId: string; ok: false; error: WireError }
  // session lifecycle
  | { type: "session.attached"; sessionId: string; capabilities: Capabilities; title: string | null; meta: SessionMeta }
  | { type: "session.window"; sessionId: string; mode: "tail" | "earlier"; window: Window } // tail replaces the transcript; earlier is prepended
  | { type: "session.append"; sessionId: string; items: TranscriptItem[] } // new items at the end
  | { type: "session.update"; sessionId: string; items: TranscriptItem[] } // full replacement of existing items by id
  | { type: "session.remove"; sessionId: string; itemId: string } // drops the item with that id; an unknown id is a no-op
  | { type: "session.delta"; sessionId: string; itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string } // text appended to blocks[blockIndex] of an assistant item
  | { type: "session.status"; sessionId: string; busy: boolean; queued: QueuedSubmission[] }
  | { type: "session.submitAck"; sessionId: string; clientSubmissionId: string; queued: boolean; submissionId?: number }
  | { type: "session.footer"; sessionId: string; footer: FooterStats }
  | { type: "session.title"; sessionId: string; title: string | null }
  | { type: "session.models"; sessionId: string; models: ModelChoice[] }
  | { type: "session.completion"; sessionId: string; completion: Completion | null }
  | { type: "session.dialog"; sessionId: string; dialog: Dialog }
  | { type: "session.dialogResolved"; sessionId: string; dialogId: string; outcome: DialogOutcome }
  | { type: "session.error"; sessionId: string | null; clientSubmissionId?: string; error: WireError }; // sessionId null: not tied to a session (invalid_message, session_not_found of an attach)

export type SessionServerMessage = Extract<ServerMessage, { type: `session.${string}` }>; // what a SessionBackend emits

// ---- the one parse of client data (CONTRACT 4.3, 5.5) ----
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

const TIER = /^[GHSO]([0-4]|X)$/;
function parseChanges(o: Obj): SettingsChanges {
  const c = objOf(o, "changes");
  const tiers: Record<string, string> = {};
  const experimental: Record<string, boolean> = {};
  if (c["tiers"] !== undefined) {
    const t = objOf(c, "tiers");
    for (const [k, v] of Object.entries(t)) {
      if (!(SETTINGS_ROLES as readonly string[]).includes(k) || typeof v !== "string" || !TIER.test(v)) throw bad(`tiers.${k} is not a Tier`, "changes");
      tiers[k] = v;
    }
  }
  if (c["experimental"] !== undefined) {
    const e = objOf(c, "experimental");
    for (const [k, v] of Object.entries(e)) {
      if (!(SETTINGS_SWITCHES as readonly string[]).includes(k) || typeof v !== "boolean") throw bad(`experimental.${k} is not a switch`, "changes");
      experimental[k] = v;
    }
  }
  return { ...(Object.keys(tiers).length > 0 ? { tiers } : {}), ...(Object.keys(experimental).length > 0 ? { experimental } : {}) };
}

const COMPLETION_LABELS: readonly string[] = ["safe_to_archive", "unanswered_ask", "open_work", "unrecorded_decision", "unescalated_issue"];
const COMPLETION_TONES: readonly string[] = ["done", "decision", "blocked"];
const refuse = (message: string, field: string): WebError => new WebError("protocol_error", message, { field });

function parseCompletion(v: unknown): Completion | null {
  if (v === null) return null;
  if (!isObj(v)) throw refuse("session.completion: completion must be an object or null", "completion");
  const { label, text, score, tone } = v;
  if (typeof label !== "string" || !COMPLETION_LABELS.includes(label)) throw refuse("session.completion: unknown label", "label");
  if (typeof text !== "string") throw refuse("session.completion: text must be a string", "text");
  if (typeof score !== "number" || !Number.isFinite(score)) throw refuse("session.completion: score must be a finite number", "score");
  if (typeof tone !== "string" || !COMPLETION_TONES.includes(tone)) throw refuse("session.completion: unknown tone", "tone");
  const base = { label, text, score, tone } as Completion;
  if (v["others"] === undefined) return base;
  const raw = v["others"];
  if (!Array.isArray(raw)) throw refuse("session.completion: others must be an array", "others");
  const others = raw.map((o: unknown) => {
    if (!isObj(o) || typeof o["label"] !== "string" || !COMPLETION_LABELS.includes(o["label"]) || typeof o["score"] !== "number" || !Number.isFinite(o["score"]))
      throw refuse("session.completion: others entries must be {label, score}", "others");
    return { label: o["label"], score: o["score"] } as CompletionScore;
  });
  return { ...base, others };
}

/** Parses an untrusted server message; only session.completion is parsed here, the rest is refused. */
export function parseServerMessage(raw: unknown): ServerMessage {
  if (!isObj(raw) || typeof raw["type"] !== "string") throw refuse("server message must be an object with a string type", "type");
  if (raw["type"] !== "session.completion") throw refuse(`unsupported server message type ${raw["type"]}`, "type");
  const sessionId = raw["sessionId"];
  if (typeof sessionId !== "string" || sessionId === "") throw refuse("session.completion: sessionId must be a non-empty string", "sessionId");
  return { type: "session.completion", sessionId, completion: parseCompletion(raw["completion"]) };
}

export function parseClientMessage(raw: unknown): ClientMessage {
  if (!isObj(raw) || typeof raw["type"] !== "string") throw bad("message must be an object with a string type", "type");
  const o = raw;
  const sessionId = (): string => nonEmpty(o, "sessionId");
  switch (o["type"]) {
    case "ping": return { type: "ping" };
    case "cockpit.subscribe": return { type: "cockpit.subscribe" };
    case "cockpit.selectPlan": {
      const planId = o["planId"];
      if (planId !== null && typeof planId !== "string") throw bad("planId must be a string or null", "planId");
      return { type: "cockpit.selectPlan", planId };
    }
    case "cockpit.action": return { type: "cockpit.action", actionId: nonEmpty(o, "actionId"), action: parseCockpitAction(o["action"]) };
    case "logs.read": return { type: "logs.read" };
    case "settings.read": return { type: "settings.read" };
    case "settings.save": return { type: "settings.save", changes: parseChanges(o) };
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
