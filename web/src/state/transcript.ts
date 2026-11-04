import { WebError, type Cursor, type TranscriptItem, type Window } from "../protocol/domain.ts";

export interface TranscriptState {
  readonly items: readonly TranscriptItem[];
  readonly hasEarlier: boolean;
  readonly cursor: Cursor | null;
  readonly tailEpoch: number;
}

export const emptyTranscript: TranscriptState = { items: [], hasEarlier: false, cursor: null, tailEpoch: 0 };

/** Moves an orphan result `orphan:<callId>` into the tool item that now has that callId. */
function adopt(items: readonly TranscriptItem[]): readonly TranscriptItem[] {
  const orphans = new Map<string, Extract<TranscriptItem, { kind: "orphanResult" }>>();
  for (const i of items) if (i.kind === "orphanResult") orphans.set(i.callId, i);
  if (orphans.size === 0) return items;
  const used = new Set<string>();
  const out = items.map((i) => {
    if (i.kind !== "tool") return i;
    const orphan = orphans.get(i.callId);
    if (orphan === undefined) return i;
    used.add(i.callId);
    if (i.result !== undefined) return i;
    return { ...i, result: orphan.result, status: orphan.result.isError ? ("failed" as const) : ("done" as const) };
  });
  return used.size === 0 ? items : out.filter((i) => !(i.kind === "orphanResult" && used.has(i.callId)));
}

function mergeById(items: readonly TranscriptItem[], incoming: readonly TranscriptItem[]): TranscriptItem[] {
  const out = [...items];
  for (const item of incoming) {
    const at = out.findIndex((i) => i.id === item.id);
    if (at >= 0) out[at] = item;
    else out.push(item);
  }
  return out;
}

export function setTail(window: Window, _previous?: TranscriptState): TranscriptState {
  return { items: adopt(window.items), hasEarlier: window.hasEarlier, cursor: window.cursor, tailEpoch: 0 };
}
export function prependEarlier(state: TranscriptState, window: Window): TranscriptState {
  const known = new Set(state.items.map((i) => i.id));
  const older = window.items.filter((i) => !known.has(i.id));
  return { items: adopt([...older, ...state.items]), hasEarlier: window.hasEarlier, cursor: window.cursor, tailEpoch: state.tailEpoch };
}
export function appendItems(state: TranscriptState, items: readonly TranscriptItem[]): TranscriptState {
  return { ...state, items: adopt(mergeById(state.items, items)) };
}
export function updateItems(state: TranscriptState, items: readonly TranscriptItem[]): TranscriptState {
  const known = new Set(state.items.map((i) => i.id));
  const loaded = items.filter((i) => known.has(i.id));
  if (loaded.length === 0) return state;
  return { ...state, items: adopt(mergeById(state.items, loaded)) };
}
export function applyDelta(
  state: TranscriptState,
  d: { itemId: string; blockIndex: number; blockType: "text" | "thinking"; text: string },
): TranscriptState {
  const at = state.items.findIndex((i) => i.id === d.itemId);
  if (at < 0) return state;
  const item = state.items[at];
  if (item === undefined || item.kind !== "assistant") {
    throw new WebError("protocol_error", `delta targets non-assistant item ${d.itemId}`, { itemId: d.itemId });
  }
  if (d.blockIndex > item.blocks.length) {
    throw new WebError("protocol_error", `delta blockIndex ${d.blockIndex} beyond ${item.blocks.length} blocks`, { itemId: d.itemId });
  }
  const blocks =
    d.blockIndex === item.blocks.length
      ? [...item.blocks, { type: d.blockType, text: d.text }]
      : item.blocks.map((b, i) => (i === d.blockIndex ? { ...b, text: b.text + d.text } : b));
  const items = [...state.items];
  items[at] = { ...item, blocks };
  return { ...state, items };
}
