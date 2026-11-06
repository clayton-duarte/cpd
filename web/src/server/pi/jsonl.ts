import { readFileSync } from "node:fs";
import { FOLD_TITLE_MAX, WebError } from "../../protocol/domain.ts";
import type {
  AssistantBlock,
  Dialog,
  FoldItem,
  FoldKind,
  Question,
  ToolItem,
  ToolResult,
  TranscriptAttachment,
  TranscriptItem,
} from "../../protocol/domain.ts";

export interface LinePosition {
  readonly line?: number;
  readonly offset: number;
}
export interface JsonlEntry {
  readonly type: string;
  readonly id?: string;
  readonly timestamp?: string;
  readonly [k: string]: unknown;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);

const malformed = (at: LinePosition, message: string): WebError =>
  new WebError("malformed_line", message, at.line === undefined ? { offset: at.offset } : { line: at.line, offset: at.offset });

export function parseLine(text: string, at: LinePosition): JsonlEntry {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (e) {
    throw malformed(at, `invalid JSON: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (!isObj(value)) throw malformed(at, "line is not a JSON object");
  if (typeof value.type !== "string") throw malformed(at, "line has no string `type`");
  return value as JsonlEntry;
}

export interface JsonlFacts {
  cwd?: string;
  sessionId?: string;
  name?: string;
  model?: string;
  thinking?: string;
  lastStopReason?: string;
  lastRole?: "user" | "assistant" | "toolResult";
}
export interface PushResult {
  added: TranscriptItem[];
  updated: TranscriptItem[];
  dialogsOpened: Dialog[];
  dialogsResolved: string[];
  factsChanged: boolean;
}
export interface PiNormalizer {
  push(text: string, at: LinePosition): PushResult;
  readonly facts: JsonlFacts;
  /** `message.timestamp` of every assistant line pushed so far, in order. */
  readonly turns: readonly number[];
}

const FOLD_KINDS: readonly string[] = ["cpd-context", "cpd-plan-decision", "cpd-plan-summary", "cpd-skills", "cpd"];

/** Text of a content value: a string as is, else its text blocks joined with "\n". */
function textOf(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((b): b is Obj => isObj(b) && b.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("\n");
}

const firstLine = (body: string): string => (body.split("\n")[0] ?? "").slice(0, FOLD_TITLE_MAX);

function foldTitle(kind: FoldKind, body: string, details: Obj): string {
  switch (kind) {
    case "cpd-context": {
      const s = str(details.summary);
      return s !== undefined && s !== "" ? s : firstLine(body);
    }
    case "cpd-plan-summary":
      return `Plan review ${String(details.review)} summary`;
    case "cpd-plan-decision":
      return `Plan review ${String(details.review)} ${details.approved === true ? "approved" : "rejected"}`;
    case "cpd-skills":
      return firstLine(body);
    case "cpd": {
      const data = isObj(details.data) ? details.data : {};
      if (details.kind === "session-message" && typeof data.from === "string") return `Message from ${data.from}`;
      const t = str(details.text);
      return firstLine(t !== undefined && t !== "" ? t : body);
    }
  }
}

const isFold = (entry: JsonlEntry): boolean =>
  entry.type === "custom_message" && FOLD_KINDS.includes(String(entry.customType)) && entry.display !== false;

/** `message.timestamp` of an assistant message; undefined for any other message. */
const turnOf = (m: Obj): number | undefined =>
  m.role === "assistant" && typeof m.timestamp === "number" && Number.isFinite(m.timestamp) ? m.timestamp : undefined;

const blocksOf = (content: unknown): Obj[] => (Array.isArray(content) ? content.filter(isObj) : []);
const hasChatBlocks = (content: unknown): boolean =>
  blocksOf(content).some((b) => b.type === "text" || b.type === "thinking");
const hasErrorMessage = (m: Obj): boolean => typeof m.errorMessage === "string" && m.errorMessage !== "";

export function countItems(entry: JsonlEntry): number {
  if (entry.type === "message" && isObj(entry.message)) {
    const m = entry.message;
    if (m.role === "user") return 1;
    if (m.role === "assistant") {
      return (hasChatBlocks(m.content) || hasErrorMessage(m) ? 1 : 0) + blocksOf(m.content).filter((b) => b.type === "toolCall").length;
    }
    return 0;
  }
  if (entry.type === "compaction") return 1;
  return isFold(entry) ? 1 : 0;
}

/** A well-formed ask_user_question `questions` argument, or null (invalid arguments open no dialog). */
function parseQuestions(raw: unknown): Question[] | null {
  if (!Array.isArray(raw) || raw.length < 1 || raw.length > 4) return null;
  const out: Question[] = [];
  for (const q of raw) {
    if (!isObj(q) || typeof q.header !== "string" || q.header.length > 16 || typeof q.question !== "string") return null;
    if (!Array.isArray(q.options) || q.options.length < 2 || q.options.length > 4) return null;
    const options = [];
    for (const o of q.options) {
      if (!isObj(o) || typeof o.label !== "string" || o.label.length > 60 || typeof o.description !== "string") return null;
      options.push(typeof o.preview === "string" ? { label: o.label, description: o.description, preview: o.preview } : { label: o.label, description: o.description });
    }
    out.push(q.multiSelect === true ? { header: q.header, question: q.question, options, multiSelect: true } : { header: q.header, question: q.question, options });
  }
  return out;
}

const ATTACHED_IMAGE_RE = /\[Attached image:\s*([^\]]+)\]/g;
const MIME_BY_EXT: Readonly<Record<string, string>> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
};

function extractInlineAttachments(rawText: string): { text: string; attachments: TranscriptAttachment[] } {
  const attachments: TranscriptAttachment[] = [];
  for (const m of rawText.matchAll(ATTACHED_IMAGE_RE)) {
    const filePath = m[1]?.trim() ?? "";
    if (filePath === "") continue;
    const ext = filePath.split(".").pop()?.toLowerCase() ?? "";
    const mimeType = MIME_BY_EXT[ext] ?? "image/png";
    let dataBase64: string | undefined;
    try {
      dataBase64 = readFileSync(filePath).toString("base64");
    } catch {
      // ignore missing file in tests or cleaned temp dir
    }
    attachments.push({
      kind: "image",
      mimeType,
      ...(dataBase64 === undefined ? {} : { dataBase64 }),
    });
  }
  const cleaned = rawText.replace(/\n*\[Attached image:\s*[^\]]+\]/g, "").trim();
  return { text: cleaned, attachments };
}

export function createNormalizer(): PiNormalizer {
  const facts: JsonlFacts = {};
  const tools = new Map<string, ToolItem>();
  const turns: number[] = [];
  const pendingAsk = new Set<string>();
  const pendingReviews = new Map<string, string>(); // dialog id -> title
  const summaries = new Map<string, string>(); // dialog id -> summary
  let unkeyedSummary = "";
  let planTitle: string | undefined;

  function push(text: string, at: LinePosition): PushResult {
    const entry = parseLine(text, at);
    const r: PushResult = { added: [], updated: [], dialogsOpened: [], dialogsResolved: [], factsChanged: false };
    const need = (cond: boolean, what: string): void => {
      if (!cond) throw malformed(at, `${entry.type} line: ${what}`);
    };
    const rawTs = entry.timestamp ?? (isObj(entry.message) ? entry.message.timestamp : undefined);
    const at_ =
      typeof rawTs === "string" && rawTs !== ""
        ? rawTs
        : typeof rawTs === "number" && Number.isFinite(rawTs)
          ? new Date(rawTs).toISOString()
          : undefined;
    const id = str(entry.id);
    const stamp = at_ === undefined ? {} : { at: at_ };
    const setFact = (key: "model" | "thinking" | "name", value: string): void => {
      if (facts[key] !== value) {
        facts[key] = value;
        r.factsChanged = true;
      }
    };

    switch (entry.type) {
      case "session":
        if (str(entry.cwd) !== undefined) facts.cwd = entry.cwd as string;
        if (id !== undefined) facts.sessionId = id;
        break;
      case "session_info":
        if (str(entry.name) !== undefined) setFact("name", entry.name as string);
        break;
      case "model_change":
        need(typeof entry.provider === "string" && typeof entry.modelId === "string", "provider and modelId must be strings");
        setFact("model", `${entry.provider as string}/${entry.modelId as string}`);
        break;
      case "thinking_level_change":
        need(typeof entry.thinkingLevel === "string", "thinkingLevel must be a string");
        setFact("thinking", entry.thinkingLevel as string);
        break;
      case "message": {
        need(isObj(entry.message) && typeof entry.message.role === "string", "message must be an object with a role");
        const m = entry.message as Obj;
        need(id !== undefined, "message needs an id");
        if (m.role === "user") {
          facts.lastRole = "user";
          const blockAttachments: TranscriptAttachment[] = blocksOf(m.content)
            .filter((b) => b.type === "image")
            .map((b) => ({ kind: "image", mimeType: str(b.mimeType) ?? "", ...(str(b.data) === undefined ? {} : { dataBase64: b.data as string }) }));
          const inline = extractInlineAttachments(textOf(m.content));
          r.added.push({ kind: "user", id: `u:${id}`, ...stamp, text: inline.text, attachments: [...blockAttachments, ...inline.attachments] });
        } else if (m.role === "assistant") {
          need(Array.isArray(m.content), "assistant content must be an array");
          facts.lastRole = "assistant";
          const turn = turnOf(m);
          if (turn !== undefined) turns.push(turn);
          const stop = str(m.stopReason);
          if (stop === undefined) delete facts.lastStopReason;
          else facts.lastStopReason = stop;
          const blocks: AssistantBlock[] = [];
          for (const b of blocksOf(m.content)) {
            if (b.type === "text" && typeof b.text === "string") blocks.push({ type: "text", text: b.text });
            else if (b.type === "thinking" && typeof b.thinking === "string" && b.thinking !== "") blocks.push({ type: "thinking", text: b.thinking });
          }
          if (hasErrorMessage(m)) blocks.push({ type: "text", text: "Error: " + (m.errorMessage as string) });
          if (blocks.length > 0) {
            r.added.push({ kind: "assistant", id: `a:${id}`, ...stamp, blocks, streaming: false, ...(str(m.model) === undefined ? {} : { model: m.model as string }) });
          }
          for (const b of blocksOf(m.content)) {
            if (b.type !== "toolCall") continue;
            need(typeof b.id === "string" && typeof b.name === "string", "toolCall needs string id and name");
            const callId = b.id as string;
            const args = isObj(b.arguments) ? b.arguments : {};
            const item: ToolItem = { kind: "tool", id: `tool:${callId}`, callId, name: b.name as string, args, status: "running" };
            tools.set(callId, item);
            r.added.push(item);
            if (item.name === "cpd_plan_review" && str(args.title) !== undefined) planTitle = args.title as string;
            if (item.name === "ask_user_question") {
              const questions = parseQuestions(args.questions);
              if (questions !== null) {
                pendingAsk.add(callId);
                r.dialogsOpened.push({ id: `call:${callId}`, method: "ask_user_question", questions, interactive: true });
              }
            }
          }
        } else if (m.role === "toolResult") {
          need(typeof m.toolCallId === "string", "toolResult needs a string toolCallId");
          facts.lastRole = "toolResult";
          const callId = m.toolCallId as string;
          const result: ToolResult = {
            text: textOf(m.content),
            isError: m.isError === true,
            ...(typeof m.durationMs === "number" ? { durationMs: m.durationMs } : {}),
          };
          const call = tools.get(callId);
          if (call === undefined) {
            r.added.push({ kind: "orphanResult", id: `orphan:${callId}`, callId, name: str(m.toolName) ?? "", result });
          } else {
            const details = isObj(m.details) ? m.details : {};
            const diff = call.name === "edit" ? (str(details.patch) ?? str(details.diff)) : undefined;
            const next: ToolItem = { ...call, result, status: result.isError ? "failed" : "done", ...(diff === undefined ? {} : { diff }) };
            tools.set(callId, next);
            r.updated.push(next);
          }
          if (pendingAsk.delete(callId)) r.dialogsResolved.push(`call:${callId}`);
        }
        break;
      }
      case "custom_message": {
        if (!isFold(entry)) break;
        need(id !== undefined, "custom_message needs an id");
        const foldKind = entry.customType as FoldKind;
        const body = textOf(entry.content);
        const details = isObj(entry.details) ? entry.details : {};
        if (foldKind === "cpd") {
          const data = isObj(details.data) ? details.data : {};
          if (details.kind === "session-message" && (data.from === null || data.unblocks === "User message from the web chat") && typeof data.text === "string" && data.text !== "") {
            facts.lastRole = "user";
            const when = str(details.at);
            const inline = extractInlineAttachments(data.text);
            r.added.push({ kind: "user", id: `u:${id}`, ...(when === undefined ? stamp : { at: when }), text: inline.text, attachments: inline.attachments });
            break;
          }
        }
        const fold: FoldItem = {
          kind: "fold",
          id: `f:${id}`,
          ...stamp,
          foldKind,
          title: foldTitle(foldKind, body, details),
          body,
          defaultOpen: foldKind === "cpd-plan-summary",
        };
        if (foldKind === "cpd-plan-summary") {
          const rv = details.review;
          if (typeof rv === "string" || typeof rv === "number") {
            const dialogId = `review:${String(rv)}`;
            summaries.set(dialogId, body);
            const title = pendingReviews.get(dialogId);
            if (title !== undefined) r.dialogsOpened.push({ id: dialogId, method: "cpd_plan_review", title, summary: body, interactive: true });
          } else unkeyedSummary = body;
        }
        r.added.push(fold);
        break;
      }
      case "compaction":
        need(typeof entry.summary === "string" && id !== undefined, "compaction needs an id and a string summary");
        r.added.push({
          kind: "compaction",
          id: `c:${id}`,
          ...stamp,
          summary: entry.summary as string,
          ...(typeof entry.tokensBefore === "number" ? { tokensBefore: entry.tokensBefore } : {}),
        });
        break;
      case "custom": {
        if (entry.customType !== "cpd-plan-review" || !isObj(entry.data)) break;
        const review = entry.data.review;
        const status = entry.data.status;
        if (typeof review !== "string" && typeof review !== "number") break;
        const dialogId = `review:${String(review)}`;
        if (status === "pending") {
          const title = str(entry.data.title) ?? planTitle ?? `Plan review ${String(review)}`;
          const summary = str(entry.data.summary) ?? summaries.get(dialogId) ?? unkeyedSummary;
          unkeyedSummary = "";
          pendingReviews.set(dialogId, title);
          r.dialogsOpened.push({ id: dialogId, method: "cpd_plan_review", title, summary, interactive: true });
        } else if (status === "approved" || status === "rejected" || status === "superseded" || status === "lost") {
          summaries.delete(dialogId);
          if (pendingReviews.delete(dialogId)) r.dialogsResolved.push(dialogId);
        }
        break;
      }
      default:
        break;
    }
    return r;
  }

  return { push, facts, turns };
}

/** Applies a push result to an ordered item list and the open dialog map (updates replace by id). */
export function applyResult(items: TranscriptItem[], open: Map<string, Dialog>, r: PushResult): void {
  items.push(...r.added);
  for (const u of r.updated) {
    const i = items.findIndex((x) => x.id === u.id);
    if (i >= 0) items[i] = u;
  }
  for (const d of r.dialogsOpened) open.set(d.id, d);
  for (const id of r.dialogsResolved) open.delete(id);
}

export function normalizeText(text: string): { items: TranscriptItem[]; facts: JsonlFacts; openDialogs: Dialog[] } {
  const n = createNormalizer();
  const items: TranscriptItem[] = [];
  const open = new Map<string, Dialog>();
  const enc = new TextEncoder();
  let offset = 0;
  text.split("\n").forEach((l, i) => {
    if (l.trim() !== "") applyResult(items, open, n.push(l, { line: i + 1, offset }));
    offset += enc.encode(l).length + 1;
  });
  return { items, facts: n.facts, openDialogs: [...open.values()] };
}

/** The `message.timestamp` of a raw assistant JSONL line; undefined for any other line. */
export function assistantTimestamp(rawLine: string): number | undefined {
  let value: unknown;
  try {
    value = JSON.parse(rawLine);
  } catch {
    return undefined;
  }
  return isObj(value) && value.type === "message" && isObj(value.message) ? turnOf(value.message) : undefined;
}
