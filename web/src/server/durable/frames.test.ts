import { describe, expect, it } from "vitest";
import type { AssistantItem, FoldItem, ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import { assertSupportedHost, createDialect } from "./frames.ts";

type Obj = Record<string, unknown>;

function entry(id: number, kind: string, message: Obj, data?: Obj): Obj {
  return { id, conversationId: "root", kind, model: [message], ...(data ? { data } : {}) };
}
function docs(over: Obj = {}): Obj {
  return {
    "pi.agent": {},
    "pi.live": {},
    "pi.inbox": { items: [] },
    "pi.provider": {},
    "pi.usage": { models: {}, tools: {} },
    ...over,
  };
}
function view(entries: Obj[], over: Obj = {}, before?: number): { view: unknown } {
  return { view: { conversation: "root", entries, ...(before === undefined ? {} : { before }), docs: docs(over) } };
}
const LIVE = ["docs", "pi.live"];
const user = (id: number, content: unknown) => entry(id, "pi.user", { role: "user", content, timestamp: 1 });
const assistant = (id: number, content: unknown[], extra: Obj = {}) =>
  entry(id, "pi.assistant", { role: "assistant", content, model: "m1", timestamp: 2, ...extra });
const result = (id: number, callId: string, text: string, isError = false, details?: Obj) =>
  entry(id, "pi.tool-result", {
    role: "toolResult",
    toolCallId: callId,
    toolName: "read",
    content: [{ type: "text", text }],
    isError,
    ...(details ? { details } : {}),
    timestamp: 3,
  });

function load(entries: Obj[], over: Obj = {}): TranscriptItem[] {
  const d = createDialect(1);
  return [...d.applyBase(view(entries, over)).transcript.items];
}
function codeOf(fn: () => unknown): unknown {
  try {
    fn();
  } catch (e) {
    return (e as { code?: unknown }).code;
  }
  return "no throw";
}
const streaming = (content: unknown[]) => ({
  "pi.live": { run: { taskId: 1, inputs: [], turns: 0 }, generation: { attempt: 1, message: { role: "assistant", content, stopReason: "pending" } } },
});

describe("durable frames dialect", () => {
  it("a view of user and assistant entries normalises to transcript items", () => {
    const items = load(
      [
        user(1, [{ type: "text", text: "a" }, { type: "text", text: "b" }, { type: "image", data: "IMG", mimeType: "image/png" }]),
        assistant(2, [{ type: "text", text: "hello" }], { usage: { totalTokens: 10 } }),
        assistant(3, [{ type: "text", text: "y" }], { usage: { totalTokens: 30 } }),
      ],
      { "pi.usage": { models: { "p/m1": { cost: { total: 0.5 } }, "p/m2": { cost: { total: 0.25 } } }, tools: {} } },
    );
    expect(items.map((i) => i.id)).toEqual(["u:1", "a:2", "a:3"]);
    expect(items[0]).toMatchObject({ kind: "user", text: "a\nb", attachments: [{ kind: "image", mimeType: "image/png", dataBase64: "IMG" }] });
    expect(items[1]).toMatchObject({ kind: "assistant", streaming: false, model: "m1", blocks: [{ type: "text", text: "hello" }] });
    const d = createDialect(1);
    const t = d.applyBase(view([assistant(2, [{ type: "text", text: "x" }], { usage: { totalTokens: 10 } }), assistant(3, [{ type: "text", text: "y" }], { usage: { totalTokens: 30 } })], { "pi.usage": { models: { "p/m1": { cost: { total: 0.5 } }, "p/m2": { cost: { total: 0.25 } } }, tools: {} } })).transcript;
    expect(t.facts.costUsd).toBeCloseTo(0.75);
    expect(t.facts.lastContextTokens).toBe(30);
    expect(t.before).toBeUndefined();
    expect(createDialect(1).applyBase(view([user(5, "x")], {}, 4)).transcript.before).toBe(4);
  });

  it("thinking blocks are kept separate from text", () => {
    const items = load([assistant(1, [{ type: "thinking", thinking: "hmm" }, { type: "text", text: "answer" }])]);
    const a = items[0] as AssistantItem;
    expect(a.blocks).toEqual([
      { type: "thinking", text: "hmm" },
      { type: "text", text: "answer" },
    ]);
  });

  it("tool calls pair with their results", () => {
    const items = load([
      assistant(1, [{ type: "toolCall", id: "c1", name: "read", arguments: { path: "a" } }]),
      result(2, "c1", "contents"),
      assistant(3, [{ type: "toolCall", id: "c2", name: "edit", arguments: {} }]),
      result(4, "c2", "bad", true, { patch: "@@ diff" }),
      result(5, "gone", "lost"),
    ]);
    expect(items.some((i) => i.kind === "assistant")).toBe(false);
    const tools = items.filter((i): i is ToolItem => i.kind === "tool");
    expect(tools[0]).toMatchObject({ id: "tool:c1", callId: "c1", name: "read", args: { path: "a" }, status: "done", result: { text: "contents", isError: false } });
    expect(tools[1]).toMatchObject({ id: "tool:c2", status: "failed", result: { isError: true } });
    expect(items.find((i) => i.kind === "orphanResult")).toMatchObject({ id: "orphan:gone", callId: "gone", result: { text: "lost" } });
  });

  it("an append op on the live message yields a text delta", () => {
    const d = createDialect(1);
    d.applyBase(view([user(1, "q")]));
    const start = d.applyIncrement({
      ops: [
        ["s", [...LIVE, "generation"], { attempt: 1 }],
        ["s", [...LIVE, "generation", "message"], { role: "assistant", content: [{ type: "text", text: "" }], stopReason: "pending" }],
      ],
    });
    const live = start.append[0] as AssistantItem;
    expect(live).toMatchObject({ kind: "assistant", streaming: true });
    expect(live.id.startsWith("live:")).toBe(true);
    const more = d.applyIncrement({ ops: [["a", [...LIVE, "generation", "message", "content", 0, "text"], "Hel"]] });
    expect(more).toMatchObject({ append: [], update: [] });
    expect(more.deltas).toEqual([{ itemId: live.id, blockIndex: 0, blockType: "text", text: "Hel" }]);
    const last = d.applyIncrement({ ops: [["a", [...LIVE, "generation", "message", "content", 0, "text"], "lo"]] });
    expect(last.deltas).toEqual([{ itemId: live.id, blockIndex: 0, blockType: "text", text: "lo" }]);
    const cached = d.transcript.items.find((i) => i.id === live.id) as AssistantItem;
    expect(cached.blocks).toEqual([{ type: "text", text: "Hello" }]);
  });

  it("cpd custom messages map to fold items", () => {
    const body = (text: string) => ({ role: "user", content: text, timestamp: 1 });
    const items = load([
      entry(1, "cpd-plan-summary", body("the summary"), { review: "r1" }),
      entry(2, "cpd-skills", body("skill one\nmore")),
      entry(3, "cpd-plan-decision", body("decided"), { review: "r1", approved: true }),
      entry(4, "cpd-context", body("ctx line\nrest"), { summary: "ctx title" }),
    ]);
    const folds = items.filter((i): i is FoldItem => i.kind === "fold");
    expect(items).toHaveLength(4);
    expect(folds[0]).toMatchObject({ id: "f:1", foldKind: "cpd-plan-summary", title: "Plan review r1 summary", body: "the summary", defaultOpen: true });
    expect(folds[1]).toMatchObject({ foldKind: "cpd-skills", title: "skill one", defaultOpen: false });
    expect(folds[2]).toMatchObject({ foldKind: "cpd-plan-decision", title: "Plan review r1 approved", defaultOpen: false });
    expect(folds[3]).toMatchObject({ foldKind: "cpd-context", title: "ctx title", defaultOpen: false });
  });

  it("compaction maps to a compaction item", () => {
    const items = load([entry(1, "pi.compaction", { role: "user", content: [{ type: "text", text: "Summary text" }], timestamp: 1 })]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "compaction", id: "c:1", summary: "Summary text" });
  });

  it("the run doc sets and clears busy", () => {
    const d = createDialect(1);
    expect(d.applyBase(view([])).transcript.facts.busy).toBe(false);
    expect(d.applyIncrement({ ops: [["s", [...LIVE, "run"], { taskId: 1, inputs: [], turns: 0 }]] }).busy).toBe(true);
    expect(d.transcript.facts.busy).toBe(true);
    expect(d.applyIncrement({ ops: [["d", [...LIVE, "run"]]] }).busy).toBe(false);
    expect(d.transcript.facts.busy).toBe(false);
  });

  it("the inbox doc sets queued and ignores write items", () => {
    const d = createDialect(1);
    d.applyBase(view([]));
    const out = d.applyIncrement({
      ops: [["s", ["docs", "pi.inbox", "items"], [{ id: 1, mode: "steer", content: "x" }, { id: 2, mode: "write", entry: 5 }, { id: 3, mode: "followUp", content: "y" }]]],
    });
    expect(out.queued).toEqual([
      { id: 1, mode: "steer" },
      { id: 3, mode: "followUp" },
    ]);
    expect(d.transcript.facts.queued).toEqual(out.queued);
    expect(d.applyIncrement({ ops: [["s", ["docs", "pi.inbox", "items"], []]] }).queued).toEqual([]);
  });

  it("the agent doc sets model and thinking", () => {
    const d = createDialect(1);
    d.applyBase(view([]));
    const out = d.applyIncrement({ ops: [["s", ["docs", "pi.agent"], { model: { provider: "anthropic", modelId: "claude" }, thinkingLevel: "high" }]] });
    expect(out.agent).toEqual({ model: "anthropic/claude", thinking: "high" });
    expect(d.transcript.facts).toMatchObject({ model: "anthropic/claude", thinking: "high" });
  });

  it("a running tool slot becomes a running tool item and its result entry completes it", () => {
    const d = createDialect(1);
    d.applyBase(view([user(1, "q")]));
    const running = d.applyIncrement({ ops: [["s", [...LIVE, "tools"], [{ callId: "c1", name: "read", status: "running" }]]] });
    expect(running.append).toHaveLength(1);
    expect(running.append[0]).toMatchObject({ kind: "tool", id: "tool:c1", callId: "c1", name: "read", status: "running" });
    const done = d.applyIncrement({
      ops: [
        ["p", ["entries"], 1, 0, [result(2, "c1", "contents")]],
        ["s", [...LIVE, "tools", 0], { callId: "c1", name: "read", status: "done", entry: 2 }],
      ],
    });
    expect(done.append).toEqual([]);
    expect(done.update).toHaveLength(1);
    expect(done.update[0]).toMatchObject({ id: "tool:c1", status: "done", result: { text: "contents", isError: false } });
  });

  it("a done tool slot without an entry becomes a failed tool item", () => {
    const d = createDialect(1);
    d.applyBase(view([user(1, "q")]));
    d.applyIncrement({ ops: [["s", [...LIVE, "tools"], [{ callId: "c1", name: "bash", status: "running" }]]] });
    const out = d.applyIncrement({ ops: [["s", [...LIVE, "tools", 0], { callId: "c1", name: "bash", status: "done" }]] });
    expect(out.update).toHaveLength(1);
    expect(out.update[0]).toMatchObject({ id: "tool:c1", status: "failed", result: { text: "", isError: true } });
  });

  it("the assistant entry replaces the live item", () => {
    const d = createDialect(1);
    d.applyBase(view([user(1, "q")], streaming([{ type: "text", text: "Hel" }])));
    const liveId = d.transcript.items[1]?.id as string;
    expect(liveId.startsWith("live:")).toBe(true);
    const out = d.applyIncrement({
      ops: [
        ["p", ["entries"], 1, 0, [assistant(2, [{ type: "text", text: "Hello" }])]],
        ["d", [...LIVE, "generation"]],
        ["d", [...LIVE, "run"]],
      ],
    });
    expect(out.append).toEqual([]);
    expect(out.busy).toBe(false);
    expect(out.update).toHaveLength(1);
    expect(out.update[0]).toMatchObject({ kind: "assistant", id: liveId, streaming: false, blocks: [{ type: "text", text: "Hello" }] });
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1", liveId]);
  });

  it("a root replacement marks the output replaced", () => {
    const d = createDialect(1);
    d.applyBase(view([user(1, "q")]));
    const next = view([user(1, "q"), assistant(2, [{ type: "text", text: "new" }])]).view;
    const out = d.applyIncrement({ ops: [["r", next]] });
    expect(out.replaced).toBe(true);
    expect(out).toMatchObject({ append: [], update: [], deltas: [] });
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1", "a:2"]);
  });

  it("an ops frame before any view raises a protocol error", () => {
    expect(codeOf(() => createDialect(1).applyIncrement({ ops: [["d", [...LIVE, "run"]]] }))).toBe("protocol_error");
  });

  it("an ops frame with a refused op raises a protocol error", () => {
    const d = createDialect(1);
    d.applyBase(view([user(1, "q")]));
    expect(codeOf(() => d.applyIncrement({ ops: [["s", ["docs", "missing", "key"], 1]] }))).toBe("protocol_error");
    expect(codeOf(() => d.applyIncrement({ ops: [["z", ["docs"]]] }))).toBe("protocol_error");
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1"]);
  });

  it("assertSupportedHost accepts the two supported protocol and version pairs", () => {
    expect(codeOf(() => assertSupportedHost({ pi_durable: "1.3.0", protocol: 1 }))).toBe("no throw");
    expect(codeOf(() => assertSupportedHost({ pi_durable: "1.5.0", protocol: 2 }))).toBe("no throw");
  });

  it("assertSupportedHost refuses mixed protocol and version pairs", () => {
    for (const [pi_durable, protocol] of [["1.5.0", 1], ["1.3.0", 2], ["1.4.0", 2], ["1.4.0", 1], ["1.5.0", 3]] as const) {
      expect(codeOf(() => assertSupportedHost({ pi_durable, protocol }))).toBe("unsupported_host_version");
    }
    let message = "";
    try {
      assertSupportedHost({ pi_durable: "1.4.0", protocol: 2 });
    } catch (e) {
      message = (e as Error).message;
    }
    // the message names both supported pairs
    expect(message).toContain("1.3.0 protocol 1");
    expect(message).toContain("1.5.0 protocol 2");
  });

  it("the v1 dialect maps a cpd-chat entry to a user item", () => {
    const items = load([entry(4, "cpd-chat", { role: "user", content: "typed in web", timestamp: 1 }, { client_id: "c1", request_id: "cs1" })]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "user", id: "u:4", text: "typed in web", attachments: [] });
  });
});

const E1 = "e1";
const baseV2 = (entries: Obj[], over: Obj = {}, before?: number) => ({ type: "view" as const, epoch: E1, seq: 1, ...view(entries, over, before) });
const opsV2 = (ops: unknown[], seq: number) => ({ type: "ops" as const, epoch: E1, seq, ops });
const LIVE_TEXT = [...LIVE, "generation", "message", "content", 0, "text"];
const startLive = [
  ["s", [...LIVE, "run"], { taskId: 1, inputs: [], turns: 0 }],
  ["s", [...LIVE, "generation"], { attempt: 1 }],
  ["s", [...LIVE, "generation", "message"], { role: "assistant", content: [{ type: "text", text: "" }], stopReason: "pending", timestamp: 2 }],
];

describe("durable frames v2 dialect", () => {
  const v2 = () => createDialect(2);
  const idle = () => [user(1, "hi"), assistant(2, [{ type: "text", text: "hello" }], { usage: { totalTokens: 10 } })];

  it("the v2 dialect turns a view into the committed transcript", () => {
    const d = v2();
    d.applyBase(baseV2(idle(), { "pi.agent": { model: { provider: "anthropic", modelId: "claude" }, thinkingLevel: "high" } }, 0));
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1", "a:2"]);
    expect(d.transcript.items[0]).toMatchObject({ kind: "user", text: "hi" });
    expect(d.transcript.items[1]).toMatchObject({ kind: "assistant", streaming: false, blocks: [{ type: "text", text: "hello" }] });
    expect(d.transcript.before).toBe(0);
    expect(d.transcript.facts).toMatchObject({ busy: false, queued: [], model: "anthropic/claude", thinking: "high", lastContextTokens: 10 });
  });

  it("the v2 dialect maps a cpd-chat entry to a user item", () => {
    const d = v2();
    d.applyBase(baseV2([entry(7, "cpd-chat", { role: "user", content: "typed in web", timestamp: 1 }, { client_id: "c1", request_id: "cs1" })]));
    expect(d.transcript.items).toHaveLength(1);
    expect(d.transcript.items[0]).toMatchObject({ kind: "user", id: "u:7", text: "typed in web", attachments: [] });
  });

  it("a v2 ops frame appending entries appends items", () => {
    const d = v2();
    d.applyBase(baseV2(idle()));
    const out = d.applyIncrement(opsV2([["p", ["entries"], 2, 0, [user(5, "more")]]], 2));
    expect(out.append.map((i) => i.id)).toEqual(["u:5"]);
    expect(out).toMatchObject({ update: [], deltas: [] });
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1", "a:2", "u:5"]);
  });

  it("a v2 text append on the live generation shows a streaming assistant item and a delta", () => {
    const d = v2();
    d.applyBase(baseV2([user(1, "hi")]));
    const start = d.applyIncrement(opsV2(startLive, 2));
    expect(start.busy).toBe(true);
    const live = start.append[0] as AssistantItem;
    expect(live).toMatchObject({ kind: "assistant", streaming: true });
    expect(live.id.startsWith("live:e1:")).toBe(true);
    const more = d.applyIncrement(opsV2([["a", LIVE_TEXT, "chunk"]], 3));
    expect(more).toMatchObject({ append: [], update: [] });
    expect(more.deltas).toEqual([{ itemId: live.id, blockIndex: 0, blockType: "text", text: "chunk" }]);
    expect((d.transcript.items.find((i) => i.id === live.id) as AssistantItem).blocks).toEqual([{ type: "text", text: "chunk" }]);
  });

  it("the committed assistant entry replaces the live item under its id", () => {
    const d = v2();
    d.applyBase(baseV2([user(1, "hi")]));
    const live = d.applyIncrement(opsV2(startLive, 2)).append[0] as AssistantItem;
    d.applyIncrement(opsV2([["a", LIVE_TEXT, "hel"]], 3));
    const out = d.applyIncrement(
      opsV2(
        [
          ["d", [...LIVE, "run"]],
          ["d", [...LIVE, "generation"]],
          ["p", ["entries"], 1, 0, [assistant(2, [{ type: "text", text: "hello world" }])]],
        ],
        4,
      ),
    );
    expect(out.busy).toBe(false);
    expect(out.append).toEqual([]);
    expect(out.update).toHaveLength(1);
    expect(out.update[0]).toMatchObject({ kind: "assistant", id: live.id, streaming: false, blocks: [{ type: "text", text: "hello world" }] });
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1", live.id]);
  });

  it("v2 busy follows the live run document", () => {
    const d = v2();
    d.applyBase(baseV2([]));
    expect(d.transcript.facts.busy).toBe(false);
    expect(d.applyIncrement(opsV2([["s", [...LIVE, "run"], { taskId: 1, inputs: [], turns: 0 }]], 2)).busy).toBe(true);
    expect(d.transcript.facts.busy).toBe(true);
    expect(d.applyIncrement(opsV2([["d", [...LIVE, "run"]]], 3)).busy).toBe(false);
    expect(d.transcript.facts.busy).toBe(false);
  });

  it("v2 queued follows the inbox document", () => {
    const d = v2();
    d.applyBase(baseV2([]));
    const out = d.applyIncrement(
      opsV2([["s", ["docs", "pi.inbox", "items"], [{ id: 1, mode: "steer", content: "x" }, { id: 2, mode: "write", entry: 5 }, { id: 3, mode: "followUp", content: "y" }]]], 2),
    );
    expect(out.queued).toEqual([{ id: 1, mode: "steer" }, { id: 3, mode: "followUp" }]);
    expect(d.transcript.facts.queued).toEqual(out.queued);
  });

  it("a v2 window drop keeps the known items", () => {
    const d = v2();
    d.applyBase(baseV2([user(1, "a"), user(2, "b"), user(3, "c")]));
    const out = d.applyIncrement(opsV2([["p", ["entries"], 0, 2, []], ["s", ["before"], 2], ["p", ["entries"], 1, 0, [user(4, "d")]]], 2));
    expect(out.append.map((i) => i.id)).toEqual(["u:4"]);
    expect(out.replaced).toBeUndefined();
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1", "u:2", "u:3", "u:4"]);
    expect(d.transcript.before).toBe(2);
  });

  it("an unknown v2 entry kind is dropped", () => {
    const d = v2();
    d.applyBase(baseV2([user(1, "a"), entry(2, "app.mystery", { role: "user", content: "x", timestamp: 1 }), entry(3, "cpd-error", { role: "user", content: "e", timestamp: 1 })]));
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1"]);
    const out = d.applyIncrement(opsV2([["p", ["entries"], 3, 0, [entry(4, "app.other", { role: "user", content: "y", timestamp: 1 })]]], 2));
    expect(out).toMatchObject({ append: [], update: [], deltas: [] });
    expect(d.transcript.items.map((i) => i.id)).toEqual(["u:1"]);
  });
});
