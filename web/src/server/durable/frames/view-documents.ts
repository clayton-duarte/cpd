import { arr, isObj, num, obj, str, type Obj } from "./validation.ts";

export type View = { entries: unknown[]; before?: number; docs: Obj };
export type Slot = { callId: string; name: string; status: string; entry?: number };
type Queued = { id: number; mode: string };
type AgentFacts = { model?: string; thinking?: string };

export function adopt(raw: unknown): View {
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
export const liveOf = (view: View): Obj => doc(view, "pi.live");
export const liveMessageOf = (view: View): Obj | undefined => {
  const generation = liveOf(view).generation;
  return isObj(generation) && isObj(generation.message) ? generation.message : undefined;
};
export const slotsOf = (view: View): Slot[] => {
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
export const queuedOf = (view: View): Queued[] => {
  const items = doc(view, "pi.inbox").items;
  if (items === undefined) return [];
  return arr(items, "inbox items")
    .map((q) => obj(q, "inbox item"))
    .filter((o) => o.mode !== "write")
    .map((o) => ({ id: num(o.id, "inbox id"), mode: str(o.mode, "inbox mode") }));
};
export const agentOf = (view: View): AgentFacts => {
  const agent = doc(view, "pi.agent");
  const out: AgentFacts = {};
  if (agent.model !== undefined) {
    const m = obj(agent.model, "agent.model");
    out.model = `${str(m.provider, "model provider")}/${str(m.modelId, "model id")}`;
  }
  if (typeof agent.thinkingLevel === "string") out.thinking = agent.thinkingLevel;
  return out;
};
export const costOf = (view: View): number => {
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
