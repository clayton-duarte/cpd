import {
  WebError,
  type AssistantItem,
  type Dialog,
  type ToolItem,
  type TranscriptItem,
} from "../../../protocol/domain.ts";
import { createReplica } from "../ops.ts";
import { arr, isObj, num, obj, protocolError, str } from "./validation.ts";
import { isoOf } from "./message-fields.ts";
import type { Dialect, DialectOutput, DurableFacts, DurableTranscript } from "./types.ts";
import { blocksOf, parseEntry, toolBlocksOf, type Parsed, type ToolBlock } from "./view-entries.ts";
import { adopt, agentOf, costOf, liveMessageOf, liveOf, queuedOf, slotsOf, type Slot, type View } from "./view-documents.ts";

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
export function createViewDialect(): ViewDialect {
  const cache = new Map<number, { raw: unknown; parsed: Parsed }>();
  const alias = new Map<number, string>(); // assistant entry id -> the live item id it replaced
  const replica = createReplica();
  let view: View | null = null;
  let items: TranscriptItem[] = [];
  let facts: DurableFacts = { busy: false, compacting: false, costUsd: 0, queued: [] };
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
      compacting: liveOf(v).compactions === undefined ? false : arr(liveOf(v).compactions, "compactions").length > 0,
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
        out.compacting = facts.compacting;
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
      if (facts.compacting !== prevFacts.compacting) out.compacting = facts.compacting;
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
