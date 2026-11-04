import { applyImmutable } from "@earendil-works/chord/delta";
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

// The single dialect seam: pi-durable 1.5.0 `view` / `ops` frames -> domain transcript (CONTRACT 6.4).

export interface DurableFacts {
  model?: string;
  thinking?: string | null;
  busy: boolean;
  costUsd: number;
  lastContextTokens?: number;
  queued: { id: number; mode: string }[];
  cwd?: string;
}
export interface DurableTranscript {
  readonly items: TranscriptItem[];
  readonly facts: DurableFacts;
  readonly before?: number;
}

export const SUPPORTED_PI_DURABLE = "1.5.0";
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void {
  if (welcome.pi_durable !== SUPPORTED_PI_DURABLE || welcome.protocol !== 2) {
    throw new WebError(
      "unsupported_host_version",
      `durable host pi-durable ${welcome.pi_durable} protocol ${welcome.protocol}; only pi-durable ${SUPPORTED_PI_DURABLE} protocol 2 is supported`,
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
export interface Gap {
  readonly after: number;
  readonly upTo: number;
}
export interface Dialect {
  applyBase(frame: { view: unknown }): { transcript: DurableTranscript };
  applyIncrement(frame: { ops: unknown[] }): DialectOutput;
  applyHistory(frame: { entries: readonly unknown[]; next?: string }): void;
  readonly gap: Gap | undefined;
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

const num = (v: unknown, what: string): number => (typeof v === "number" && Number.isFinite(v) ? v : protocolError(`${what} is not a number`));

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
    case "cpd":
      return firstLine(body);
  }
}

/** Text and thinking blocks of an assistant message; a live (partial) message may lack fields. */
function blocksOf(message: Obj, live: boolean): AssistantBlock[] {
  const blocks: AssistantBlock[] = [];
  for (const raw of arr(message.content, "assistant content")) {
    const b = obj(raw, "assistant block");
    if (b.type === "text") blocks.push({ type: "text", text: live && typeof b.text !== "string" ? "" : str(b.text, "text block") });
    else if (b.type === "thinking") blocks.push({ type: "thinking", text: live && typeof b.thinking !== "string" ? "" : str(b.thinking, "thinking block") });
  }
  return blocks;
}

type ToolBlock = { callId: string; name: string; args: Obj };
const toolBlocksOf = (message: Obj, live: boolean): ToolBlock[] =>
  arr(message.content, "assistant content")
    .filter((b): b is Obj => isObj(b) && b.type === "toolCall")
    .map((b) => ({
      callId: str(b.id, "toolCall id"),
      name: live && typeof b.name !== "string" ? "" : str(b.name, "toolCall name"),
      args: isObj(b.arguments) ? b.arguments : {},
    }));

type Parsed =
  | { kind: "item"; item: TranscriptItem }
  | { kind: "assistant"; item?: AssistantItem; tools: ToolBlock[]; tokens?: number }
  | { kind: "result"; callId: string; name: string; result: { text: string; isError: boolean }; patch?: string }
  | { kind: "none" };

function parseEntry(rawEntry: unknown): { id: number; parsed: Parsed } {
  const entry = obj(rawEntry, "entry");
  const id = num(entry.id, "entry id");
  const kind = str(entry.kind, "entry kind");
  const messageOf = (role?: string): Obj => {
    const message = obj(arr(entry.model, `entry ${id} model`)[0], `entry ${id} model[0]`);
    if (role !== undefined && message.role !== role) protocolError(`entry ${id}: expected role ${role}, got ${String(message.role)}`);
    return message;
  };
  if (kind === "pi.user") {
    const message = messageOf("user");
    const attachments: TranscriptAttachment[] = Array.isArray(message.content)
      ? message.content
          .filter((b): b is Obj => isObj(b) && b.type === "image")
          .map((b) => ({ kind: "image", mimeType: str(b.mimeType, "image mimeType"), dataBase64: str(b.data, "image data") }))
      : [];
    return { id, parsed: { kind: "item", item: { kind: "user", id: `u:${id}`, ...isoOf(message), text: textOf(message.content), attachments } } };
  }
  if (kind === "pi.assistant") {
    const message = messageOf("assistant");
    const blocks = blocksOf(message, false);
    const usage = isObj(message.usage) ? message.usage : undefined;
    return {
      id,
      parsed: {
        kind: "assistant",
        tools: toolBlocksOf(message, false),
        ...(typeof usage?.totalTokens === "number" ? { tokens: usage.totalTokens } : {}),
        ...(blocks.length > 0
          ? { item: { kind: "assistant", id: `a:${id}`, ...isoOf(message), blocks, streaming: false, ...(typeof message.model === "string" ? { model: message.model } : {}) } }
          : {}),
      },
    };
  }
  if (kind === "pi.tool-result") {
    const message = messageOf("toolResult");
    const details = isObj(message.details) ? message.details : {};
    const patch = typeof details.patch === "string" ? details.patch : typeof details.diff === "string" ? details.diff : undefined;
    return {
      id,
      parsed: {
        kind: "result",
        callId: str(message.toolCallId, "toolCallId"),
        name: str(message.toolName, "toolName"),
        result: { text: textOf(message.content), isError: message.isError === true },
        ...(patch === undefined ? {} : { patch }),
      },
    };
  }
  if (kind === "pi.compaction") {
    const message = messageOf();
    return { id, parsed: { kind: "item", item: { kind: "compaction", id: `c:${id}`, ...isoOf(message), summary: textOf(message.content) } } };
  }
  if ((FOLD_KINDS as readonly string[]).includes(kind)) {
    const message = messageOf();
    const body = textOf(message.content);
    const foldKind = kind as FoldKind;
    const fold: FoldItem = {
      kind: "fold",
      id: `f:${id}`,
      ...isoOf(message),
      foldKind,
      title: foldTitle(foldKind, body, isObj(entry.data) ? entry.data : {}),
      body,
      defaultOpen: foldKind === "cpd-plan-summary",
    };
    return { id, parsed: { kind: "item", item: fold } };
  }
  // pi.system, pi.reset, cpd-chat, cpd-event, cpd-first-message, cpd-error, cpd-plan-review and any other kind: dropped (CONTRACT 6.4)
  return { id, parsed: { kind: "none" } };
}

type View = { entries: unknown[]; before?: number; docs: Obj };
type Slot = { callId: string; name: string; status: string; entry?: number };
type Queued = { id: number; mode: string };
type AgentFacts = { model?: string; thinking?: string };

function adopt(raw: unknown): View {
  const v = obj(raw, "view");
  const entries = arr(v.entries, "view.entries");
  const docs = obj(v.docs, "view.docs");
  if (v.before !== undefined) num(v.before, "view.before");
  return { entries, docs, ...(v.before === undefined ? {} : { before: v.before as number }) };
}

const doc = (view: View, name: string): Obj => (isObj(view.docs[name]) ? (view.docs[name] as Obj) : {});
const liveOf = (view: View): Obj => doc(view, "pi.live");
const liveMessageOf = (view: View): Obj | undefined => {
  const generation = liveOf(view).generation;
  return isObj(generation) && isObj(generation.message) ? generation.message : undefined;
};
const slotsOf = (view: View): Slot[] => {
  const tools = liveOf(view).tools;
  if (tools === undefined) return [];
  return arr(tools, "pi.live tools").map((t) => {
    const o = obj(t, "tool slot");
    return {
      callId: str(o.callId, "slot callId"),
      name: str(o.name, "slot name"),
      status: str(o.status, "slot status"),
      ...(typeof o.entry === "number" ? { entry: o.entry } : {}),
    };
  });
};
const queuedOf = (view: View): Queued[] => {
  const items = doc(view, "pi.inbox").items;
  if (items === undefined) return [];
  return arr(items, "inbox items")
    .map((q) => obj(q, "inbox item"))
    .filter((o) => o.mode !== "write")
    .map((o) => ({ id: num(o.id, "inbox id"), mode: str(o.mode, "inbox mode") }));
};
const agentOf = (view: View): AgentFacts => {
  const agent = doc(view, "pi.agent");
  const out: AgentFacts = {};
  if (agent.model !== undefined) {
    const m = obj(agent.model, "agent.model");
    out.model = `${str(m.provider, "model provider")}/${str(m.modelId, "model id")}`;
  }
  if (typeof agent.thinkingLevel === "string") out.thinking = agent.thinkingLevel;
  return out;
};
const costOf = (view: View): number => {
  const models = doc(view, "pi.usage").models;
  if (!isObj(models)) return 0;
  let total = 0;
  for (const usage of Object.values(models)) {
    const cost = isObj(usage) && isObj(usage.cost) ? usage.cost.total : undefined;
    if (typeof cost === "number") total += cost;
  }
  return total;
};

const same = (a: unknown, b: unknown): boolean => a === b || JSON.stringify(a) === JSON.stringify(b);

type Delta = DialectOutput["deltas"][number];

function createV1Dialect(): Dialect {
  const cache = new Map<number, { raw: unknown; parsed: Parsed }>();
  const alias = new Map<number, string>(); // assistant entry id -> the live item id it replaced
  let view: View | null = null;
  let items: TranscriptItem[] = [];
  let facts: DurableFacts = { busy: false, costUsd: 0, queued: [] };
  let liveId: string | null = null;
  let liveCount = 0;
  let entryTokens: number | undefined;

  /** Parse every entry of the view that the cache has not seen (or sees changed); returns the ids first seen. */
  const ingest = (entries: readonly unknown[]): number[] => {
    const fresh: number[] = [];
    for (const raw of entries) {
      const probe = obj(raw, "entry");
      const known = cache.get(num(probe.id, "entry id"));
      if (known !== undefined && known.raw === raw) continue;
      const { id, parsed } = parseEntry(raw);
      if (known === undefined) fresh.push(id);
      cache.set(id, { raw, parsed });
    }
    return fresh;
  };

  const toolItem = (block: ToolBlock, slot: Slot | undefined, result: Extract<Parsed, { kind: "result" }> | undefined): ToolItem => {
    const base = { kind: "tool", id: `tool:${block.callId}`, callId: block.callId, name: block.name, args: block.args } as const;
    if (result !== undefined) {
      const diff = block.name === "edit" ? result.patch : undefined;
      return { ...base, result: result.result, status: result.result.isError ? "failed" : "done", ...(diff === undefined ? {} : { diff }) };
    }
    if (slot?.status === "done" && slot.entry === undefined) return { ...base, result: { text: "", isError: true }, status: "failed" };
    return { ...base, status: "running" };
  };

  const liveItem = (v: View): AssistantItem | undefined => {
    const message = liveMessageOf(v);
    if (message === undefined || liveId === null) return undefined;
    const blocks = blocksOf(message, true);
    if (blocks.length === 0) return undefined;
    return { kind: "assistant", id: liveId, ...isoOf(message), blocks, streaming: true, ...(typeof message.model === "string" ? { model: message.model } : {}) };
  };

  const derive = (v: View): TranscriptItem[] => {
    const ids = [...cache.keys()].sort((a, b) => a - b);
    const slots = new Map(slotsOf(v).map((s) => [s.callId, s]));
    const liveMessage = liveMessageOf(v);
    const liveTools = liveMessage === undefined ? [] : toolBlocksOf(liveMessage, true);
    const results = new Map<string, Extract<Parsed, { kind: "result" }>>();
    const called = new Set<string>();
    for (const id of ids) {
      const p = (cache.get(id) as { parsed: Parsed }).parsed;
      if (p.kind === "result") results.set(p.callId, p);
      else if (p.kind === "assistant") for (const t of p.tools) called.add(t.callId);
    }
    for (const t of liveTools) called.add(t.callId);
    for (const callId of slots.keys()) called.add(callId);

    const out: TranscriptItem[] = [];
    const emitted = new Set<string>();
    const tool = (block: ToolBlock): void => {
      if (emitted.has(block.callId)) return;
      emitted.add(block.callId);
      out.push(toolItem(block, slots.get(block.callId), results.get(block.callId)));
    };
    entryTokens = undefined;
    for (const id of ids) {
      const p = (cache.get(id) as { parsed: Parsed }).parsed;
      switch (p.kind) {
        case "item":
          out.push(p.item);
          break;
        case "assistant": {
          if (p.tokens !== undefined) entryTokens = p.tokens;
          const to = alias.get(id);
          if (p.item !== undefined) out.push(to === undefined ? p.item : { ...p.item, id: to });
          for (const t of p.tools) tool(t);
          break;
        }
        case "result":
          if (!called.has(p.callId)) out.push({ kind: "orphanResult", id: `orphan:${p.callId}`, callId: p.callId, name: p.name, result: p.result });
          break;
        case "none":
          break;
      }
    }
    const live = liveItem(v);
    if (live !== undefined) out.push(live);
    for (const t of liveTools) tool({ ...t, name: t.name === "" ? (slots.get(t.callId)?.name ?? "") : t.name });
    for (const s of slots.values()) tool({ callId: s.callId, name: s.name, args: {} });
    return out;
  };

  const factsOf = (v: View): DurableFacts => {
    const agent = agentOf(v);
    const usage = liveMessageOf(v)?.usage;
    const streaming = isObj(usage) && typeof usage.totalTokens === "number" ? usage.totalTokens : undefined;
    const tokens = streaming ?? entryTokens;
    return {
      busy: liveOf(v).run !== undefined,
      costUsd: costOf(v),
      queued: queuedOf(v),
      ...(agent.model === undefined ? {} : { model: agent.model }),
      ...(agent.thinking === undefined ? {} : { thinking: agent.thinking }),
      ...(tokens === undefined ? {} : { lastContextTokens: tokens }),
    };
  };

  const transcript: DurableTranscript = {
    get items() {
      return items;
    },
    get facts() {
      return facts;
    },
    get before() {
      return view?.before;
    },
  };

  const protocolFrom = (e: unknown, what: string): WebError =>
    e instanceof WebError ? e : new WebError("protocol_error", `${what}: ${e instanceof Error ? e.message : String(e)}`);

  /** 'a' ops appending to the text or thinking of a live message content block. */
  const deltasOf = (ops: readonly unknown[], v: View): Delta[] => {
    const message = liveMessageOf(v);
    if (message === undefined || liveId === null) return [];
    const content = Array.isArray(message.content) ? message.content : [];
    const out: Delta[] = [];
    for (const op of ops) {
      if (!Array.isArray(op) || op[0] !== "a" || !Array.isArray(op[1]) || typeof op[2] !== "string") continue;
      const p = op[1] as unknown[];
      const field = p[6];
      if (p.length !== 7 || p[0] !== "docs" || p[1] !== "pi.live" || p[2] !== "generation" || p[3] !== "message" || p[4] !== "content") continue;
      if (typeof p[5] !== "number" || (field !== "text" && field !== "thinking")) continue;
      let blockIndex = 0;
      for (let i = 0; i < p[5]; i++) {
        const b = content[i];
        if (isObj(b) && (b.type === "text" || b.type === "thinking")) blockIndex++;
      }
      out.push({ itemId: liveId, blockIndex, blockType: field, text: op[2] });
    }
    return out;
  };

  const isLiveAppend = (op: unknown): boolean => {
    if (!Array.isArray(op) || op[0] !== "a" || !Array.isArray(op[1])) return false;
    const p = op[1] as unknown[];
    return p.length === 7 && p[0] === "docs" && p[1] === "pi.live" && p[2] === "generation" && p[3] === "message" && p[4] === "content";
  };

  const settleLive = (v: View, fresh: number[]): boolean => {
    if (liveMessageOf(v) !== undefined) {
      liveId ??= `live:${++liveCount}`;
      return false;
    }
    if (liveId === null) return false;
    const finished = fresh.find((id) => (cache.get(id) as { parsed: Parsed }).parsed.kind === "assistant");
    const orphaned = finished === undefined;
    if (finished !== undefined) alias.set(finished, liveId);
    liveId = null;
    return orphaned;
  };

  return {
    transcript,
    get gap() {
      if (view === null || view.before === undefined) return undefined;
      const first = view.entries.length === 0 ? undefined : num(obj(view.entries[0], "entry").id, "entry id");
      if (first === undefined) return undefined;
      let below: number | undefined;
      for (const id of cache.keys()) if (id < first && (below === undefined || id > below)) below = id;
      return below === undefined || below >= view.before ? undefined : { after: below, upTo: view.before };
    },
    applyBase(frame) {
      try {
        const next = adopt(frame.view);
        const fresh = ingest(next.entries);
        view = next;
        settleLive(next, fresh);
        items = derive(next);
        facts = factsOf(next);
      } catch (e) {
        throw protocolFrom(e, "view");
      }
      return { transcript };
    },
    applyIncrement(frame) {
      const out: DialectOutput = { append: [], update: [], deltas: [] };
      if (view === null) return protocolError("ops frame before any view");
      const ops = arr(frame.ops, "ops");
      let next: View;
      let nextItems: TranscriptItem[];
      let nextFacts: DurableFacts;
      let deltas: Delta[];
      let removed = false;
      const before = view;
      const prevLive = liveId;
      const prevItems = items;
      try {
        next = adopt(applyImmutable(before as never, ops as never));
        const fresh = ingest(next.entries);
        view = next;
        removed = settleLive(next, fresh);
        deltas = deltasOf(ops, next);
        const liveOnly = !removed && prevLive !== null && liveId === prevLive && ops.length > 0 && ops.every(isLiveAppend) && fresh.length === 0;
        if (liveOnly) {
          const live = liveItem(next);
          const at = prevItems.findIndex((i) => i.id === prevLive);
          nextItems = live !== undefined && at >= 0 ? prevItems.map((i, k) => (k === at ? live : i)) : derive(next);
        } else {
          nextItems = derive(next);
        }
        nextFacts = factsOf(next);
      } catch (e) {
        view = before;
        liveId = prevLive;
        throw protocolFrom(e, "ops");
      }
      items = nextItems;
      const prevFacts = facts;
      facts = nextFacts;

      const replaced = (Array.isArray(ops[0]) && ops[0][0] === "r") || removed;
      const prevById = new Map(prevItems.map((i) => [i.id, i]));
      const nextIds = new Set(nextItems.map((i) => i.id));
      const vanished = prevItems.some((i) => !nextIds.has(i.id));
      if (replaced || vanished) {
        out.replaced = true;
        out.busy = facts.busy;
        out.queued = facts.queued;
        out.agent = { ...(facts.model === undefined ? {} : { model: facts.model }), ...(facts.thinking === undefined ? {} : { thinking: facts.thinking }) };
        return out;
      }
      for (const item of nextItems) {
        const old = prevById.get(item.id);
        if (old === undefined) out.append.push(item);
        else if (!same(old, item)) out.update.push(item);
      }
      const appended = new Set(out.append.map((i) => i.id));
      // An update that only adds the delta text to the live item is the delta alone.
      out.update = out.update.filter((item) => {
        const own = deltas.filter((d) => d.itemId === item.id);
        const old = prevById.get(item.id);
        if (own.length === 0 || old?.kind !== "assistant" || item.kind !== "assistant") return true;
        const grown = own.reduce((blocks, d) => blocks.map((b, i) => (i === d.blockIndex ? { ...b, text: b.text + d.text } : b)), old.blocks);
        return !same({ ...old, blocks: grown }, item);
      });
      const updated = new Set(out.update.map((i) => i.id));
      out.deltas = deltas.filter((d) => !appended.has(d.itemId) && !updated.has(d.itemId));
      if (facts.busy !== prevFacts.busy) out.busy = facts.busy;
      if (!same(facts.queued, prevFacts.queued)) out.queued = facts.queued;
      if (facts.model !== prevFacts.model || facts.thinking !== prevFacts.thinking) {
        out.agent = { ...(facts.model === undefined ? {} : { model: facts.model }), ...(facts.thinking === undefined ? {} : { thinking: facts.thinking }) };
      }
      return out;
    },
    applyHistory(frame) {
      try {
        ingest(arr(frame.entries, "history entries"));
        if (view !== null) {
          items = derive(view);
          facts = factsOf(view);
        }
      } catch (e) {
        throw protocolFrom(e, "history");
      }
    },
  };
}
export function createDialect(protocol: 1 | 2): Dialect {
  if (protocol === 1) return createV1Dialect();
  const notImplemented = (): never => {
    throw new Error("not implemented");
  };
  return {
    get transcript(): DurableTranscript {
      return notImplemented();
    },
    get gap(): Gap | undefined {
      return notImplemented();
    },
    applyBase: notImplemented,
    applyIncrement: notImplemented,
    applyHistory: notImplemented,
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
