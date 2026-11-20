import type { AssistantBlock, AssistantItem, FoldItem, FoldKind, ToolItem, TranscriptAttachment, TranscriptItem } from "../../../protocol/domain.ts";
import { FOLD_KINDS, foldTitle, isoOf, skillsBaseline, skillsMessage, textOf } from "./message-fields.ts";
import type { Dialect, DialectOutput, DurableFacts, DurableTranscript } from "./types.ts";
import { arr, isObj, num, obj, protocolError, str, type Obj } from "./validation.ts";

const idOf = (entry: Obj): string => {
  const id = entry.id;
  return typeof id === "string" || (typeof id === "number" && Number.isFinite(id)) ? String(id) : protocolError("entry id is not a string or number");
};

type EventsLive = { id: string; created: boolean; blocks: Map<number, number>; toolIds: Map<number, string> };

export function createEventsDialect(): Dialect {
  const items: TranscriptItem[] = [];
  const facts: DurableFacts = { busy: false, compacting: false, costUsd: 0, queued: [] };
  const transcript: DurableTranscript = { items, facts, before: undefined };
  const index = new Map<string, number>(); // item id -> position in items
  const alias = new Map<string, string>(); // `a:<entry id>` -> live id
  const costed = new Set<string>();
  let live: EventsLive | null = null;
  let liveCount = 0;
  const compactions = new Set<number>();
  let failureId: string | undefined;

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
  const clearFailure = (out: Out): void => {
    if (failureId === undefined) return;
    const at = index.get(failureId);
    if (at !== undefined) {
      items.splice(at, 1);
      index.clear();
      items.forEach((item, i) => index.set(item.id, i));
      out.replaced = true;
    }
    failureId = undefined;
  };
  const putFailure = (raw: unknown, out: Out): void => {
    const failure = obj(raw, "compaction failure");
    const id = `cf:${num(failure.taskId, "compaction taskId")}`;
    if (failureId !== id) clearFailure(out);
    failureId = id;
    put({ kind: "fold", id, foldKind: "cpd", title: `Compaction failed (${str(failure.reason, "compaction reason")})`, body: str(failure.error, "compaction error"), defaultOpen: true }, out);
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
      clearFailure(out);
      const message = obj(arr(entry.model, "compaction model")[0], "compaction model[0]");
      put({ kind: "compaction", id: `c:${entryId}`, ...isoOf(message), summary: textOf(message.content) }, out);
    } else if (skillsBaseline(entry)) {
      // The initial skill digest is bookkeeping with no display message.
    } else if ((FOLD_KINDS as readonly string[]).includes(kind)) {
      const message = obj(arr(entry.model, "fold model")[0], "fold model[0]");
      if (kind === "cpd-skills") skillsMessage(message);
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
    compactions.clear();
    for (const raw of arr(snap.compactions ?? [], "snapshot.compactions")) compactions.add(num(obj(raw, "compaction").taskId, "compaction taskId"));
    facts.compacting = compactions.size > 0;
    failureId = undefined;
    live = null;
    facts.costUsd = 0;
    delete facts.lastContextTokens;
    const sink = newOut();
    for (const e of entries) ingest(e, sink);
    if (snap.compactionFailure !== undefined) putFailure(snap.compactionFailure, sink);
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

  const NO_OUTPUT = new Set(["turn_start", "turn_end", "submission", "auto_retry_start", "auto_retry_end", "deferred_poll", "task_failed", "tool_execution_update", "usage_changed"]);

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
        out.compacting = facts.compacting;
        return;
      }
      case "compaction_start":
        compactions.add(num(event.taskId, "compaction taskId"));
        facts.compacting = compactions.size > 0;
        out.compacting = facts.compacting;
        return;
      case "compaction_end":
        compactions.delete(num(event.taskId, "compaction taskId"));
        facts.compacting = compactions.size > 0;
        out.compacting = facts.compacting;
        if (event.error !== undefined) putFailure(event, out);
        return;
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
