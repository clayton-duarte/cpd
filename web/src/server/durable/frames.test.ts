import { describe, expect, it } from "vitest";
import type { AssistantItem, FoldItem, ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import { assertSupportedHost, createDialect } from "./frames.ts";

type Obj = Record<string, unknown>;

function entry(id: string, kind: string, message: Obj, data?: Obj): Obj {
  return { id, conversationId: "root", kind, model: [message], ...(data ? { data } : {}) };
}
function snapshot(entries: Obj[]): { snapshot: unknown } {
  return { snapshot: { type: "snapshot", entries, tools: {}, nestedTools: {}, compactions: [], inbox: [], agent: {} } };
}
const user = (id: string, content: unknown) => entry(id, "pi.user", { role: "user", content, timestamp: 1 });
const assistant = (id: string, content: unknown[], extra: Obj = {}) =>
  entry(id, "pi.assistant", { role: "assistant", content, model: "m1", timestamp: 2, ...extra });
const result = (id: string, callId: string, text: string, isError = false, details?: Obj) =>
  entry(id, "pi.tool-result", {
    role: "toolResult",
    toolCallId: callId,
    toolName: "read",
    content: [{ type: "text", text }],
    isError,
    ...(details ? { details } : {}),
    timestamp: 3,
  });

function load(entries: Obj[]): TranscriptItem[] {
  const d = createDialect();
  return [...d.applySnapshot(snapshot(entries)).transcript.items];
}
function codeOf(fn: () => unknown): unknown {
  try {
    fn();
  } catch (e) {
    return (e as { code?: unknown }).code;
  }
  return "no throw";
}

describe("durable frames dialect", () => {
  it("user and assistant messages normalise to transcript items", () => {
    const items = load([
      user("e1", [{ type: "text", text: "a" }, { type: "text", text: "b" }, { type: "image", data: "IMG", mimeType: "image/png" }]),
      assistant("e2", [{ type: "text", text: "hello" }], { usage: { totalTokens: 10, cost: { total: 0.5 } } }),
    ]);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({
      kind: "user",
      id: "u:e1",
      text: "a\nb",
      attachments: [{ kind: "image", mimeType: "image/png", dataBase64: "IMG" }],
    });
    expect(items[1]).toMatchObject({ kind: "assistant", id: "a:e2", streaming: false, model: "m1", blocks: [{ type: "text", text: "hello" }] });
    const d = createDialect();
    const t = d.applySnapshot(
      snapshot([assistant("e2", [{ type: "text", text: "x" }], { usage: { totalTokens: 10, cost: { total: 0.5 } } }), assistant("e3", [{ type: "text", text: "y" }], { usage: { totalTokens: 30, cost: { total: 0.25 } } })]),
    ).transcript;
    expect(t.facts.costUsd).toBeCloseTo(0.75);
    expect(t.facts.lastContextTokens).toBe(30);
  });

  it("thinking blocks are kept separate from text", () => {
    const items = load([assistant("e1", [{ type: "thinking", thinking: "hmm" }, { type: "text", text: "answer" }])]);
    const a = items[0] as AssistantItem;
    expect(a.blocks).toEqual([
      { type: "thinking", text: "hmm" },
      { type: "text", text: "answer" },
    ]);
  });

  it("tool calls pair with their results", () => {
    const items = load([
      assistant("e1", [{ type: "toolCall", id: "c1", name: "read", arguments: { path: "a" } }]),
      result("e2", "c1", "contents"),
      assistant("e3", [{ type: "toolCall", id: "c2", name: "edit", arguments: {} }]),
      result("e4", "c2", "bad", true, { patch: "@@ diff" }),
      result("e5", "gone", "lost"),
    ]);
    expect(items.some((i) => i.kind === "assistant")).toBe(false);
    const tools = items.filter((i): i is ToolItem => i.kind === "tool");
    expect(tools[0]).toMatchObject({ id: "tool:c1", callId: "c1", name: "read", args: { path: "a" }, status: "done", result: { text: "contents", isError: false } });
    expect(tools[1]).toMatchObject({ id: "tool:c2", status: "failed", result: { isError: true } });
    expect(items.find((i) => i.kind === "orphanResult")).toMatchObject({ id: "orphan:gone", callId: "gone", result: { text: "lost" } });
  });

  it("streaming text deltas append to the assistant item", () => {
    const d = createDialect();
    d.applySnapshot(snapshot([]));
    const start = d.applyEvents({
      events: [
        { type: "message_start", message: { role: "assistant", content: [] } },
        { type: "message_update", changes: [{ type: "text_start", contentIndex: 0, block: { type: "text", text: "" } }] },
      ],
    });
    const live = start.append[0] as AssistantItem;
    expect(live).toMatchObject({ kind: "assistant", streaming: true });
    expect(live.id.startsWith("live:")).toBe(true);
    const more = d.applyEvents({
      events: [
        { type: "message_update", changes: [{ type: "text_delta", contentIndex: 0, delta: "Hel" }] },
        { type: "message_update", changes: [{ type: "text_delta", contentIndex: 0, delta: "lo" }] },
      ],
    });
    expect(more.deltas).toEqual([
      { itemId: live.id, blockIndex: 0, blockType: "text", text: "Hel" },
      { itemId: live.id, blockIndex: 0, blockType: "text", text: "lo" },
    ]);
    const cached = d.transcript.items.find((i) => i.id === live.id) as AssistantItem;
    expect(cached.blocks).toEqual([{ type: "text", text: "Hello" }]);
  });

  it("cpd custom messages map to fold items", () => {
    const body = (text: string) => ({ role: "user", content: text, timestamp: 1 });
    const items = load([
      entry("e1", "cpd-plan-summary", body("the summary"), { review: "r1" }),
      entry("e2", "cpd-skills", body("skill one\nmore")),
      entry("e3", "cpd-plan-decision", body("decided"), { review: "r1", approved: true }),
      entry("e4", "cpd-context", body("ctx line\nrest"), { summary: "ctx title" }),
      entry("e5", "cpd-chat", body("dropped")),
    ]);
    const folds = items.filter((i): i is FoldItem => i.kind === "fold");
    expect(items).toHaveLength(4);
    expect(folds[0]).toMatchObject({ id: "f:e1", foldKind: "cpd-plan-summary", title: "Plan review r1 summary", body: "the summary", defaultOpen: true });
    expect(folds[1]).toMatchObject({ foldKind: "cpd-skills", title: "skill one", defaultOpen: false });
    expect(folds[2]).toMatchObject({ foldKind: "cpd-plan-decision", title: "Plan review r1 approved", defaultOpen: false });
    expect(folds[3]).toMatchObject({ foldKind: "cpd-context", title: "ctx title", defaultOpen: false });
  });

  it("compaction maps to a compaction item", () => {
    const items = load([entry("e1", "pi.compaction", { role: "user", content: [{ type: "text", text: "Summary text" }], timestamp: 1 })]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "compaction", id: "c:e1", summary: "Summary text" });
  });

  it("an unknown frame shape raises a typed protocol error", () => {
    const d = createDialect();
    d.applySnapshot(snapshot([]));
    expect(codeOf(() => d.applyEvents({ events: [{ type: "bogus_event" }] }))).toBe("protocol_error");
    expect(codeOf(() => d.applyEvents({ events: ["not an object"] }))).toBe("protocol_error");
    expect(codeOf(() => createDialect().applySnapshot(snapshot([{ kind: "pi.user", model: [] }])))).toBe("protocol_error");
  });

  it("an unsupported host version raises unsupported_host_version", () => {
    expect(codeOf(() => assertSupportedHost({ pi_durable: "1.5.0", protocol: 1 }))).toBe("unsupported_host_version");
    expect(codeOf(() => assertSupportedHost({ pi_durable: "1.3.0", protocol: 1 }))).toBe("no throw");
  });
});
