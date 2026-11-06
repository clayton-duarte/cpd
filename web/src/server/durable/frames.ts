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
import { createReplica } from "./ops.ts";

// The dialect seam (CONTRACT 10.8): durable-host protocol 1 (pi-durable 1.3.0 `snapshot` / `events`) and
// protocol 2 (pi-durable 1.5.0 `view` / `ops`) -> domain transcript.

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
  readonly openDialog?: Dialog | undefined;
}

export const SUPPORTED_HOSTS: readonly { protocol: 1 | 2; piDurable: string }[] = [
  { protocol: 1, piDurable: "1.3.0" },
  { protocol: 2, piDurable: "1.5.0" },
];
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void {
  if (!SUPPORTED_HOSTS.some((h) => h.protocol === welcome.protocol && h.piDurable === welcome.pi_durable)) {
    throw new WebError(
      "unsupported_host_version",
      `durable host pi-durable ${welcome.pi_durable} protocol ${welcome.protocol}; only ${SUPPORTED_HOSTS.map((h) => `pi-durable ${h.piDurable} protocol ${h.protocol}`).join(" and ")} are supported`,
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
/** A `snapshot` (protocol 1) or `view` (protocol 2) frame; epoch and seq are the protocol 2 stream cursor. */
export type BaseFrame = { snapshot: unknown } | { view: unknown; epoch?: string; seq?: number };
/** An `events` (protocol 1) or `ops` (protocol 2) frame. */
export type IncrementFrame = { events: unknown[] } | { ops: unknown[]; epoch?: string; seq?: number };
export interface Dialect {
  applyBase(frame: BaseFrame): { transcript: DurableTranscript };
  applyIncrement(frame: IncrementFrame): DialectOutput;
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
    else if (b.type === "thinking") {
      const text = live && typeof b.thinking !== "string" ? "" : str(b.thinking, "thinking block");
      if (text !== "") blocks.push({ type: "thinking", text });
    }
  }
  if (typeof message.errorMessage === "string" && message.errorMessage !== "") blocks.push({ type: "text", text: "Error: " + message.errorMessage });
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
  | { kind: "review"; review: string; status: string }
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
  if (kind === "pi.user" || kind === "cpd-chat") {
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
    const foldData =
      isObj(entry.data) && Object.keys(entry.data).length > 0
        ? entry.data
        : isObj(message.details)
          ? message.details
          : {};
    const fold: FoldItem = {
      kind: "fold",
      id: `f:${id}`,
      ...isoOf(message),
      foldKind,
      title: foldTitle(foldKind, body, foldData),
      body,
      defaultOpen: foldKind === "cpd-plan-summary",
    };
    return { id, parsed: { kind: "item", item: fold } };
  }
  if (kind === "cpd-plan-review" && isObj(entry.data)) {
    const review = entry.data.review;
    const status = entry.data.status;
    if ((typeof review === "string" || typeof review === "number") && typeof status === "string") {
      return { id, parsed: { kind: "review", review: String(review), status } };
    }
  }
  // pi.system, pi.reset, cpd-event, cpd-first-message, cpd-error and any other kind: dropped (CONTRACT 6.4)
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

// An absent document is unknown; a present one that is not an object is a protocol error.
const doc = (view: View, name: string): Obj => {
  const d = view.docs[name];
  return d === undefined ? {} : obj(d, `document ${name}`);
};
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
  const usage = doc(view, "pi.usage");
  let total = 0;
  for (const part of [usage.models, usage.tools]) {
    if (!isObj(part)) continue;
    for (const u of Object.values(part)) {
      const cost = isObj(u) && isObj(u.cost) ? u.cost.total : undefined;
      if (typeof cost === "number") total += cost;
    }
  }
  return total;
};

const same = (a: unknown, b: unknown): boolean => a === b || JSON.stringify(a) === JSON.stringify(b);

type Delta = DialectOutput["deltas"][number];

/*
 * The protocol 2 dialect: `view` / `ops` frames folded through the ops replica (scout-v2shape.md b, c, e).
 * Left out, because the artefact marks it unverified or CONTRACT 10.9 puts it out of scope:
 * - `history` paging beyond the host's newest-50 window (the gap fill is typed but untested against a real host);
 * - live tool-call streaming ops, multi-round tool runs and window overflow ops (mocked frames only);
 * - `host_state.name` as the title, `runFailure` / `generation.retry` display;
 * - `pi.reset` dividers, `cpd-event` / `cpd-first-message` entries and tool progress output;
 * - footer cost counts `pi.usage` models and tools `cost.total` as documented; whether tools overlap models is unverified.
 */
type ViewFrame = { epoch: string; seq: number; view: unknown };
type OpsFrame = { epoch: string; seq: number; ops: unknown[] };
type ViewDialect = Omit<Dialect, "applyBase" | "applyIncrement"> & {
  applyBase(frame: ViewFrame): { transcript: DurableTranscript };
  applyIncrement(frame: OpsFrame): DialectOutput;
};
function createViewDialect(): ViewDialect {
  const cache = new Map<number, { raw: unknown; parsed: Parsed }>();
  const alias = new Map<number, string>(); // assistant entry id -> the live item id it replaced
  const replica = createReplica();
  let view: View | null = null;
  let items: TranscriptItem[] = [];
  let facts: DurableFacts = { busy: false, costUsd: 0, queued: [] };
  let liveId: string | null = null;
  let liveCount = 0;
  let entryTokens: number | undefined;
  let openDialog: Dialog | undefined;

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
    const pendingReviews = new Set<string>();
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
        case "review":
          if (p.status === "pending") pendingReviews.add(p.review);
          else if (p.status === "approved" || p.status === "rejected" || p.status === "superseded" || p.status === "lost") pendingReviews.delete(p.review);
          break;
        case "none":
          break;
      }
    }
    const live = liveItem(v);
    if (live !== undefined) out.push(live);
    for (const t of liveTools) tool({ ...t, name: t.name === "" ? (slots.get(t.callId)?.name ?? "") : t.name });
    for (const s of slots.values()) tool({ callId: s.callId, name: s.name, args: {} });
    const failure = liveOf(v).compactionFailure;
    if (isObj(failure)) {
      out.push({
        kind: "fold",
        id: `cf:${String(failure.taskId)}`,
        foldKind: "cpd",
        title: `Compaction failed (${str(failure.trigger, "compactionFailure trigger")})`,
        body: str(failure.error, "compactionFailure error"),
        defaultOpen: true,
      });
    }
    const latestReview = [...pendingReviews].at(-1);
    if (latestReview !== undefined) {
      let title = `Plan review ${latestReview}`;
      let summary = "";
      for (const item of [...out].reverse()) {
        if (item.kind === "tool" && item.name === "cpd_plan_review" && typeof item.args.title === "string") {
          title = item.args.title;
          break;
        }
      }
      for (const item of [...out].reverse()) {
        if (item.kind === "fold" && item.foldKind === "cpd-plan-summary") {
          summary = item.body;
          break;
        }
      }
      openDialog = { id: `review:${latestReview}`, method: "cpd_plan_review", title, summary, interactive: true };
    } else {
      openDialog = undefined;
    }
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
    get openDialog() {
      return openDialog;
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
      liveId ??= `live:${replica.cursor?.epoch ?? ""}:${++liveCount}`;
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
    applyBase(frame: ViewFrame) {
      try {
        const next = adopt(frame.view);
        replica.view(frame);
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
    applyIncrement(frame: OpsFrame) {
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
        replica.ops({ epoch: frame.epoch, seq: frame.seq, ops });
        next = adopt(replica.value);
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
const idOf = (entry: Obj): string => {
  const id = entry.id;
  return typeof id === "string" || (typeof id === "number" && Number.isFinite(id)) ? String(id) : protocolError("entry id is not a string or number");
};

type EventsLive = { id: string; created: boolean; blocks: Map<number, number>; toolIds: Map<number, string> };

function createEventsDialect(): Dialect {
  const items: TranscriptItem[] = [];
  const facts: DurableFacts = { busy: false, costUsd: 0, queued: [] };
  const transcript: DurableTranscript = { items, facts, before: undefined };
  const index = new Map<string, number>(); // item id -> position in items
  const alias = new Map<string, string>(); // `a:<entry id>` -> live id
  const costed = new Set<string>();
  let live: EventsLive | null = null;
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
    const model = arr(entry.model, `entry ${idOf(entry)} model`);
    const message = obj(model[0], `entry ${idOf(entry)} model[0]`);
    if (message.role !== role) protocolError(`entry ${idOf(entry)}: expected role ${role}, got ${String(message.role)}`);
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
    const entryId = idOf(entry);
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
    const entryId = idOf(entry);
    const kind = str(entry.kind, "entry kind");
    if (kind === "pi.user" || kind === "cpd-chat") {
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
      const foldData =
        isObj(entry.data) && Object.keys(entry.data).length > 0
          ? entry.data
          : isObj(message.details)
            ? message.details
            : {};
      const fold: FoldItem = {
        kind: "fold",
        id: `f:${entryId}`,
        ...isoOf(message),
        foldKind,
        title: foldTitle(foldKind, body, foldData),
        body,
        defaultOpen: foldKind === "cpd-plan-summary",
      };
      put(fold, out);
    }
    // pi.system, pi.reset, cpd-event, cpd-first-message, cpd-error, cpd-plan-review and any other kind: dropped (CONTRACT 6.4)
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
    facts.busy = snap.run !== undefined;
    return { queued, agent, busy: facts.busy };
  };

  const ensureLive = (): EventsLive => (live ??= { id: `live:${++liveCount}`, created: false, blocks: new Map(), toolIds: new Map() });

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
        facts.busy = true;
        out.busy = true;
        return;
      case "run_end":
        facts.busy = false;
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
    gap: undefined, // a protocol 1 snapshot holds the whole transcript: no window, no history
    applyBase(frame) {
      if (!("snapshot" in frame)) return protocolError("expected a snapshot frame");
      reset(frame.snapshot);
      return { transcript };
    },
    applyHistory() {},
    applyIncrement(frame) {
      if (!("events" in frame)) return protocolError("expected an events frame");
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


/** Frames of the other protocol (and the frames of a bare test) carry no epoch or seq: fill the stream cursor in. */
function createViewDialectFor(protocol: 1 | 2): Dialect {
  const inner = createViewDialect();
  let epoch = "";
  let seq = 0;
  return {
    get transcript() {
      return inner.transcript;
    },
    get gap() {
      return inner.gap;
    },
    applyBase(frame) {
      if (!("view" in frame)) return protocolError(`expected a view frame on protocol ${protocol}`);
      const next = { epoch: frame.epoch ?? "v1", seq: frame.seq ?? 1 };
      const out = inner.applyBase({ view: frame.view, ...next });
      ({ epoch, seq } = next);
      return out;
    },
    applyIncrement(frame) {
      if (!("ops" in frame)) return protocolError(`expected an ops frame on protocol ${protocol}`);
      const out = inner.applyIncrement({ ops: frame.ops, epoch: frame.epoch ?? epoch, seq: frame.seq ?? seq + 1 });
      seq = frame.seq ?? seq + 1;
      return out;
    },
    applyHistory: (frame) => inner.applyHistory(frame),
  };
}

export function createDialect(protocol: 1 | 2): Dialect {
  if (protocol === 2) return createViewDialectFor(2);
  const events = createEventsDialect();
  // Protocol 1 hosts only send snapshot/events; view-shaped frames reach the shared view fold (Phase 1 unit tests feed it these).
  const view = createViewDialectFor(1);
  let active: Dialect = events;
  return {
    get transcript() {
      return active.transcript;
    },
    get gap() {
      return active.gap;
    },
    applyBase(frame) {
      active = "view" in frame ? view : events;
      return active.applyBase(frame);
    },
    applyIncrement(frame) {
      active = "ops" in frame ? view : active;
      return active.applyIncrement(frame);
    },
    applyHistory: (frame) => active.applyHistory(frame),
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
  if (req.method === "select" && (req.purpose === "secret_approve" || req.purpose === "host_approve")) {
    const title = typeof req.title === "string" ? req.title : "Approval";
    return {
      id: req.id,
      method: "ask_user_question",
      questions: [
        {
          header: "Approval",
          question: title,
          options: [
            { label: "Allow once", description: "" },
            { label: "Deny", description: "" },
          ],
        },
      ],
      interactive: true,
    };
  }
  if (req.method === "editor") return { kind: "editor", id: req.id };
  return { kind: "unsupported", id: req.id, method: req.method, purpose: req.purpose };
}
