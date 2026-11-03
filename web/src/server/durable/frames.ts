import {
  FOLD_TITLE_MAX,
  WebError,
  type AssistantBlock,
  type AssistantItem,
  type Dialog,
  type FoldItem,
  type FoldKind,
  type Question,
  type ToolItem,
  type TranscriptAttachment,
  type TranscriptItem,
} from "../../protocol/domain.ts";
import type { HostFrame } from "./client.ts";

// The single dialect seam: pi-durable 1.3.0 `snapshot` / `events` frames -> domain transcript (CONTRACT 6.4).

export interface DurableFacts {
  model?: string;
  thinking?: string | null;
  costUsd: number;
  lastContextTokens?: number;
  queued: { id: number; mode: string }[];
  cwd?: string;
}
export interface DurableTranscript {
  readonly items: TranscriptItem[];
  readonly facts: DurableFacts;
}

export const SUPPORTED_PI_DURABLE = "1.3.0";
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void {
  if (welcome.pi_durable !== SUPPORTED_PI_DURABLE || welcome.protocol !== 1) {
    throw new WebError(
      "unsupported_host_version",
      `durable host pi-durable ${welcome.pi_durable} protocol ${welcome.protocol}; only pi-durable ${SUPPORTED_PI_DURABLE} protocol 1 is supported`,
      { piDurable: welcome.pi_durable, protocol: welcome.protocol },
    );
  }
}

export interface DialectOutput {
  append: TranscriptItem[];
  update: TranscriptItem[];
  deltas: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string }[];
  busy?: boolean;
  queued?: { id: number; mode: string }[];
  agent?: { model?: string; thinking?: string | null };
  replaced?: boolean;
}
export interface Dialect {
  applySnapshot(frame: { snapshot: unknown }): { transcript: DurableTranscript };
  applyEvents(frame: { events: unknown[] }): DialectOutput;
  readonly transcript: DurableTranscript;
}

type Obj = Record<string, unknown>;
const protocolError = (message: string): never => {
  throw new WebError("protocol_error", message);
};
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const obj = (v: unknown, what: string): Obj => (isObj(v) ? v : protocolError(`${what} is not an object`));
const str = (v: unknown, what: string): string => (typeof v === "string" ? v : protocolError(`${what} is not a string`));
const arr = (v: unknown, what: string): unknown[] => (Array.isArray(v) ? v : protocolError(`${what} is not an array`));
const num = (v: unknown, what: string): number => (typeof v === "number" ? v : protocolError(`${what} is not a number`));

const isoOf = (message: Obj): { at?: string } =>
  typeof message.timestamp === "number" && Number.isFinite(message.timestamp) ? { at: new Date(message.timestamp).toISOString() } : {};

/** Text blocks of a message `content` (a string as is), joined with "\n". */
function textOf(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((b): b is Obj => isObj(b) && b.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("\n");
}

const cut = (s: string): string => s.slice(0, FOLD_TITLE_MAX);
const firstLine = (s: string): string => cut(s.split("\n")[0] ?? "");

const FOLD_KINDS: readonly FoldKind[] = ["cpd-context", "cpd-plan-decision", "cpd-plan-summary", "cpd-skills"];

function foldTitle(kind: FoldKind, body: string, data: Obj): string {
  switch (kind) {
    case "cpd-context":
      return typeof data.summary === "string" && data.summary !== "" ? cut(data.summary) : firstLine(body);
    case "cpd-plan-summary":
      return `Plan review ${String(data.review)} summary`;
    case "cpd-plan-decision":
      return `Plan review ${String(data.review)} ${data.approved === true ? "approved" : "rejected"}`;
    case "cpd-skills":
      return firstLine(body);
  }
}

type Live = { id: string; created: boolean; blocks: Map<number, number>; toolIds: Map<number, string> };

export function createDialect(): Dialect {
  const items: TranscriptItem[] = [];
  const facts: DurableFacts = { costUsd: 0, queued: [] };
  const transcript: DurableTranscript = { items, facts };
  const index = new Map<string, number>(); // item id -> position in items
  const alias = new Map<string, string>(); // `a:<entry id>` -> live id
  const costed = new Set<string>();
  let live: Live | null = null;
  let liveCount = 0;

  type Out = DialectOutput;
  const newOut = (): Out => ({ append: [], update: [], deltas: [] });

  const put = (item: TranscriptItem, out: Out): void => {
    const at = index.get(item.id);
    if (at === undefined) {
      index.set(item.id, items.length);
      items.push(item);
      out.append.push(item);
    } else {
      items[at] = item;
      out.update.push(item);
    }
  };
  const get = (id: string): TranscriptItem | undefined => {
    const at = index.get(id);
    return at === undefined ? undefined : items[at];
  };
  const getAssistant = (id: string): AssistantItem => {
    const found = get(id);
    return found?.kind === "assistant" ? found : protocolError(`no assistant item ${id}`);
  };

  const messageOf = (entry: Obj, role: string): Obj => {
    const model = arr(entry.model, `entry ${String(entry.id)} model`);
    const message = obj(model[0], `entry ${String(entry.id)} model[0]`);
    if (message.role !== role) protocolError(`entry ${String(entry.id)}: expected role ${role}, got ${String(message.role)}`);
    return message;
  };

  const toolFrom = (block: Obj): ToolItem => ({
    kind: "tool",
    id: `tool:${str(block.id, "toolCall id")}`,
    callId: str(block.id, "toolCall id"),
    name: str(block.name, "toolCall name"),
    args: isObj(block.arguments) ? block.arguments : {},
    status: "running",
  });
  const putTool = (fresh: ToolItem, out: Out): void => {
    const old = get(fresh.id);
    put(old?.kind === "tool" ? { ...fresh, ...(old.result ? { result: old.result, status: old.status } : {}), ...(old.diff ? { diff: old.diff } : {}) } : fresh, out);
  };

  const blocksOf = (message: Obj): AssistantBlock[] => {
    const blocks: AssistantBlock[] = [];
    for (const raw of arr(message.content, "assistant content")) {
      const b = obj(raw, "assistant block");
      if (b.type === "text") blocks.push({ type: "text", text: str(b.text, "text block") });
      else if (b.type === "thinking") blocks.push({ type: "thinking", text: str(b.thinking, "thinking block") });
    }
    return blocks;
  };

  const ingestAssistant = (entry: Obj, out: Out, id: string, streaming: boolean): void => {
    const message = messageOf(entry, "assistant");
    const blocks = blocksOf(message);
    const entryId = str(entry.id, "entry id");
    if (blocks.length > 0 || get(id) !== undefined) {
      put(
        {
          kind: "assistant",
          id,
          ...isoOf(message),
          blocks,
          streaming,
          ...(typeof message.model === "string" ? { model: message.model } : {}),
        },
        out,
      );
    }
    for (const raw of arr(message.content, "assistant content")) {
      if (isObj(raw) && raw.type === "toolCall") putTool(toolFrom(raw), out);
    }
    if (!costed.has(entryId) && isObj(message.usage)) {
      costed.add(entryId);
      const cost = isObj(message.usage.cost) ? message.usage.cost.total : undefined;
      if (typeof cost === "number") facts.costUsd += cost;
      if (typeof message.usage.totalTokens === "number") facts.lastContextTokens = message.usage.totalTokens;
    }
  };

  const ingest = (rawEntry: unknown, out: Out, liveId?: string): void => {
    const entry = obj(rawEntry, "entry");
    const entryId = str(entry.id, "entry id");
    const kind = str(entry.kind, "entry kind");
    if (kind === "pi.user") {
      const message = messageOf(entry, "user");
      const attachments: TranscriptAttachment[] = Array.isArray(message.content)
        ? message.content
            .filter((b): b is Obj => isObj(b) && b.type === "image")
            .map((b) => ({ kind: "image", mimeType: str(b.mimeType, "image mimeType"), dataBase64: str(b.data, "image data") }))
        : [];
      put({ kind: "user", id: `u:${entryId}`, ...isoOf(message), text: textOf(message.content), attachments }, out);
    } else if (kind === "pi.assistant") {
      const id = liveId ?? alias.get(`a:${entryId}`) ?? `a:${entryId}`;
      if (liveId !== undefined) alias.set(`a:${entryId}`, liveId);
      ingestAssistant(entry, out, id, false);
    } else if (kind === "pi.tool-result") {
      const message = messageOf(entry, "toolResult");
      const callId = str(message.toolCallId, "toolCallId");
      const isError = message.isError === true;
      const result = { text: textOf(message.content), isError };
      const tool = get(`tool:${callId}`);
      if (tool?.kind === "tool") {
        const details = isObj(message.details) ? message.details : {};
        const diff = tool.name === "edit" ? (typeof details.patch === "string" ? details.patch : typeof details.diff === "string" ? details.diff : undefined) : undefined;
        put({ ...tool, result, status: isError ? "failed" : "done", ...(diff === undefined ? {} : { diff }) }, out);
      } else {
        put({ kind: "orphanResult", id: `orphan:${callId}`, callId, name: str(message.toolName, "toolName"), result }, out);
      }
    } else if (kind === "pi.compaction") {
      const message = obj(arr(entry.model, "compaction model")[0], "compaction model[0]");
      put({ kind: "compaction", id: `c:${entryId}`, ...isoOf(message), summary: textOf(message.content) }, out);
    } else if ((FOLD_KINDS as readonly string[]).includes(kind)) {
      const message = obj(arr(entry.model, "fold model")[0], "fold model[0]");
      const body = textOf(message.content);
      const foldKind = kind as FoldKind;
      const fold: FoldItem = {
        kind: "fold",
        id: `f:${entryId}`,
        ...isoOf(message),
        foldKind,
        title: foldTitle(foldKind, body, isObj(entry.data) ? entry.data : {}),
        body,
        defaultOpen: foldKind === "cpd-plan-summary",
      };
      put(fold, out);
    }
    // pi.system, pi.reset, cpd-chat, cpd-event, cpd-first-message, cpd-error, cpd-plan-review and any other kind: dropped (CONTRACT 6.4)
  };

  const queuedOf = (raw: unknown): { id: number; mode: string }[] =>
    arr(raw, "inbox").map((q) => {
      const o = obj(q, "inbox item");
      return { id: num(o.id, "inbox id"), mode: str(o.mode, "inbox mode") };
    });
  const agentOf = (raw: unknown): { model?: string; thinking?: string | null } => {
    const agent = obj(raw, "agent");
    const out: { model?: string; thinking?: string | null } = {};
    if (agent.model !== undefined) {
      const m = obj(agent.model, "agent.model");
      out.model = `${str(m.provider, "model provider")}/${str(m.modelId, "model id")}`;
    }
    if (typeof agent.thinkingLevel === "string") out.thinking = agent.thinkingLevel;
    return out;
  };
  const setAgent = (agent: { model?: string; thinking?: string | null }): void => {
    if (agent.model !== undefined) facts.model = agent.model;
    if (agent.thinking !== undefined) facts.thinking = agent.thinking;
  };

  const reset = (raw: unknown): { queued: { id: number; mode: string }[]; agent: { model?: string; thinking?: string | null }; busy: boolean } => {
    const snap = obj(raw, "snapshot");
    if (snap.type !== "snapshot") protocolError(`snapshot.type is ${String(snap.type)}`);
    const entries = arr(snap.entries, "snapshot.entries");
    const queued = queuedOf(snap.inbox);
    const agent = agentOf(snap.agent ?? {});
    items.length = 0;
    index.clear();
    alias.clear();
    costed.clear();
    live = null;
    facts.costUsd = 0;
    delete facts.lastContextTokens;
    const sink = newOut();
    for (const e of entries) ingest(e, sink);
    facts.queued = queued;
    setAgent(agent);
    return { queued, agent, busy: snap.run !== undefined };
  };

  const ensureLive = (): Live => (live ??= { id: `live:${++liveCount}`, created: false, blocks: new Map(), toolIds: new Map() });

  const applyChange = (raw: unknown, out: Out): void => {
    const change = obj(raw, "message change");
    const type = str(change.type, "change type");
    const l = ensureLive();
    switch (type) {
      case "text_start":
      case "thinking_start": {
        const at = num(change.contentIndex, "contentIndex");
        const block = obj(change.block, "change block");
        const blockType = type === "text_start" ? "text" : "thinking";
        const text = blockType === "text" ? str(block.text ?? "", "text") : str(block.thinking ?? "", "thinking");
        const fresh: AssistantBlock = { type: blockType, text };
        if (!l.created) {
          l.created = true;
          l.blocks.set(at, 0);
          put({ kind: "assistant", id: l.id, blocks: [fresh], streaming: true }, out);
        } else {
          const item = getAssistant(l.id);
          l.blocks.set(at, item.blocks.length);
          put({ ...item, blocks: [...item.blocks, fresh] }, out);
        }
        return;
      }
      case "text_delta":
      case "thinking_delta": {
        const at = num(change.contentIndex, "contentIndex");
        const blockIndex = l.blocks.get(at);
        if (blockIndex === undefined || !l.created) return protocolError(`${type} for unstarted block ${at}`);
        const item = getAssistant(l.id);
        const delta = str(change.delta, "delta");
        const blocks = item.blocks.map((b, i) => (i === blockIndex ? { ...b, text: b.text + delta } : b));
        items[index.get(l.id) as number] = { ...item, blocks };
        out.deltas.push({ itemId: l.id, blockIndex, blockType: type === "text_delta" ? "text" : "thinking", text: delta });
        return;
      }
      case "toolcall_start": {
        const tool = toolFrom(obj(change.block, "change block"));
        l.toolIds.set(num(change.contentIndex, "contentIndex"), tool.callId);
        putTool({ ...tool, args: {} }, out);
        return;
      }
      case "toolcall_delta":
        return;
      case "block": {
        const at = num(change.contentIndex, "contentIndex");
        const block = obj(change.block, "change block");
        if (block.type === "toolCall") {
          putTool(toolFrom(block), out);
          return;
        }
        const blockIndex = l.blocks.get(at);
        if (blockIndex === undefined || !l.created) return;
        const item = getAssistant(l.id);
        const text = block.type === "thinking" ? str(block.thinking, "thinking") : str(block.text, "text");
        put({ ...item, blocks: item.blocks.map((b, i) => (i === blockIndex ? { ...b, text } : b)) }, out);
        return;
      }
      case "message": {
        const message = obj(change.message, "change message");
        const blocks = blocksOf(message);
        l.blocks.clear();
        let n = 0;
        arr(message.content, "content").forEach((b, i) => {
          if (isObj(b) && (b.type === "text" || b.type === "thinking")) l.blocks.set(i, n++);
        });
        if (blocks.length > 0 || l.created) {
          l.created = true;
          put({ kind: "assistant", id: l.id, ...isoOf(message), blocks, streaming: true }, out);
        }
        for (const b of arr(message.content, "content")) if (isObj(b) && b.type === "toolCall") putTool(toolFrom(b), out);
        return;
      }
      default:
        return protocolError(`unknown message change ${type}`);
    }
  };

  const NO_OUTPUT = new Set(["turn_start", "turn_end", "submission", "auto_retry_start", "auto_retry_end", "deferred_poll", "task_failed", "compaction_start", "compaction_end", "tool_execution_update", "usage_changed"]);

  const applyEvent = (raw: unknown, out: Out): void => {
    const event = obj(raw, "event");
    const type = str(event.type, "event type");
    switch (type) {
      case "snapshot": {
        const r = reset(event);
        out.replaced = true;
        out.queued = r.queued;
        out.agent = r.agent;
        out.busy = r.busy;
        return;
      }
      case "run_start":
        out.busy = true;
        return;
      case "run_end":
        out.busy = false;
        return;
      case "message_start": {
        const message = obj(event.message, "message");
        if (message.role === "assistant") live = { id: `live:${++liveCount}`, created: false, blocks: new Map(), toolIds: new Map() };
        return;
      }
      case "message_update":
        for (const c of arr(event.changes, "changes")) applyChange(c, out);
        return;
      case "message_end": {
        const l = live;
        live = null;
        ingest(event.entry, out, l?.created ? l.id : undefined);
        return;
      }
      case "entry_appended":
        ingest(event.entry, out);
        return;
      case "tool_execution_start": {
        if (event.parentToolCallId !== undefined) return;
        const callId = str(event.toolCallId, "toolCallId");
        const old = get(`tool:${callId}`);
        const args = isObj(event.args) ? event.args : {};
        if (old?.kind === "tool") put({ ...old, args }, out);
        else put({ kind: "tool", id: `tool:${callId}`, callId, name: str(event.toolName, "toolName"), args, status: "running" }, out);
        return;
      }
      case "tool_execution_end": {
        if (event.parentToolCallId !== undefined) return;
        if (event.entry !== undefined) {
          ingest(event.entry, out);
          return;
        }
        const old = get(`tool:${str(event.toolCallId, "toolCallId")}`);
        if (old?.kind === "tool") put({ ...old, status: "failed", result: { text: "", isError: true } }, out);
        return;
      }
      case "inbox_update": {
        out.queued = queuedOf(event.items);
        facts.queued = out.queued;
        return;
      }
      case "agent_changed": {
        const agent = agentOf(event.agent);
        out.agent = agent;
        setAgent(agent);
        return;
      }
      default:
        if (!NO_OUTPUT.has(type)) protocolError(`unknown event ${type}`);
    }
  };

  return {
    transcript,
    applySnapshot(frame) {
      reset(frame.snapshot);
      return { transcript };
    },
    applyEvents(frame) {
      const out = newOut();
      for (const e of frame.events) applyEvent(e, out);
      // Final state of every touched item; deltas of touched items are already inside them.
      const final = (list: TranscriptItem[]): TranscriptItem[] => list.map((i) => get(i.id) ?? i);
      const dedupe = (list: TranscriptItem[]): TranscriptItem[] => list.filter((i, at) => list.findIndex((j) => j.id === i.id) === at);
      out.append = dedupe(final(out.append));
      const appended = new Set(out.append.map((i) => i.id));
      out.update = dedupe(final(out.update)).filter((i) => !appended.has(i.id));
      const touched = new Set([...appended, ...out.update.map((i) => i.id)]);
      out.deltas = out.deltas.filter((d) => !touched.has(d.itemId));
      return out;
    },
  };
}

function questionsOf(raw: unknown): Question[] {
  return arr(raw, "questions").map((q) => {
    const o = obj(q, "question");
    const options = arr(o.options, "question options").map((opt) => {
      const x = obj(opt, "question option");
      return { label: str(x.label, "option label"), description: str(x.description ?? "", "option description"), ...(typeof x.preview === "string" ? { preview: x.preview } : {}) };
    });
    return { header: str(o.header, "question header"), question: str(o.question, "question text"), options, ...(o.multiSelect === true ? { multiSelect: true } : {}) };
  });
}

export function dialogFromRequest(
  req: Extract<HostFrame, { type: "ui_request" }>,
  transcript: readonly TranscriptItem[],
): Dialog | { kind: "editor"; id: string } | { kind: "unsupported"; id: string; method: string; purpose: string } {
  if (req.method === "questionnaire") {
    return { id: req.id, method: "ask_user_question", questions: questionsOf(req.questions), interactive: true };
  }
  if (req.method === "select" && req.purpose === "plan_review" && JSON.stringify(req.options) === '["Approve","Change"]') {
    let title = "Plan review";
    let summary = "";
    for (const item of [...transcript].reverse()) {
      if (item.kind === "tool" && item.name === "cpd_plan_review" && typeof item.args.title === "string") {
        title = item.args.title;
        break;
      }
    }
    for (const item of [...transcript].reverse()) {
      if (item.kind === "fold" && item.foldKind === "cpd-plan-summary") {
        summary = item.body;
        break;
      }
    }
    return { id: req.id, method: "cpd_plan_review", title, summary, interactive: true };
  }
  if (req.method === "editor") return { kind: "editor", id: req.id };
  return { kind: "unsupported", id: req.id, method: req.method, purpose: req.purpose };
}
