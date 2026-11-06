import { describe, expect, it } from "vitest";
import type { AssistantItem, ToolItem } from "../../protocol/domain.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { applyDelta, appendItems, emptyTranscript, type TranscriptState } from "../../state/transcript.ts";
import { createLiveSource, type LiveConnection } from "./live.ts";

class FakeConn implements LiveConnection {
  closed = 0;
  private line: (l: string) => void = () => {};
  private close_: () => void = () => {};
  onLine(cb: (l: string) => void): void {
    this.line = cb;
  }
  onClose(cb: () => void): void {
    this.close_ = cb;
  }
  close(): void {
    this.closed += 1;
  }
  feed(frame: unknown): void {
    this.line(typeof frame === "string" ? frame : JSON.stringify(frame));
  }
  drop(): void {
    this.close_();
  }
}

function setup(opts: { absent?: number; committed?: (ts: number) => boolean } = {}) {
  const scheduler = new FakeScheduler();
  const paths: string[] = [];
  const conns: FakeConn[] = [];
  let absent = opts.absent ?? 0;
  const live = createLiveSource({
    cpdHome: "/home",
    scheduler,
    connect: (path) => {
      paths.push(path);
      if (absent > 0) {
        absent -= 1;
        throw new Error("ENOENT");
      }
      const c = new FakeConn();
      conns.push(c);
      return c;
    },
  });
  const out: SessionServerMessage[] = [];
  live.attach("s1", (m) => out.push(m), opts.committed ?? (() => false));
  return { scheduler, paths, conns, live, out };
}
const conn = (s: ReturnType<typeof setup>): FakeConn => s.conns[0] as FakeConn;
const hello = { t: "hello", protocol: 1, session: "s1" };
const idle = { t: "inflight", message: null, tools: [] };
const start = (ts: number, extra: object = {}) => ({ t: "message_start", message: { role: "assistant", content: [], timestamp: ts, ...extra } });
const textDelta = (index: number, delta: string) => ({ t: "delta", kind: "text", phase: "delta", index, delta });
const begun = (opts?: Parameters<typeof setup>[0]) => {
  const s = setup(opts);
  conn(s).feed(hello);
  conn(s).feed(idle);
  return s;
};

describe("live source", () => {
  it("connects to the session socket path under cpd home", () => {
    const s = setup();
    expect(s.paths).toEqual(["/home/run/pi/s1.sock"]);
    expect(s.out).toEqual([]);
  });

  it("retries every second while the socket is absent", () => {
    const s = setup({ absent: 2 });
    expect(s.paths).toHaveLength(1);
    s.scheduler.advance(999);
    expect(s.paths).toHaveLength(1);
    s.scheduler.advance(1);
    expect(s.paths).toHaveLength(2);
    s.scheduler.advance(1000);
    expect(s.paths).toHaveLength(3);
    expect(s.conns).toHaveLength(1);
    s.scheduler.advance(5000);
    expect(s.paths).toHaveLength(3);
    expect(s.out).toEqual([]);
    conn(s).drop();
    s.scheduler.advance(1000);
    expect(s.paths).toHaveLength(4);
  });

  it("stops retrying and closes on detach", () => {
    const s = setup({ absent: 5 });
    s.live.detach();
    expect(s.scheduler.pending()).toBe(0);
    s.scheduler.advance(10_000);
    expect(s.paths).toHaveLength(1);
    const t = setup();
    t.live.detach();
    expect(conn(t).closed).toBe(1);
    expect(t.scheduler.pending()).toBe(0);
  });

  it("inflight with a partial assistant message appends a streaming live item", () => {
    const s = setup();
    conn(s).feed(hello);
    conn(s).feed({
      t: "inflight",
      message: { role: "assistant", model: "m/1", timestamp: 100, content: [{ type: "thinking", thinking: "hm" }, { type: "text", text: "Hi" }, { type: "toolCall", id: "c9", name: "read", arguments: {} }] },
      tools: [],
    });
    const item: AssistantItem = {
      kind: "assistant",
      id: "live:100",
      blocks: [{ type: "thinking", text: "hm" }, { type: "text", text: "Hi" }],
      streaming: true,
      model: "m/1",
    };
    expect(s.out).toEqual([{ type: "session.append", sessionId: "s1", items: [item] }]);
    conn(s).feed(textDelta(2, " there"));
    expect(s.out[1]).toEqual({ type: "session.delta", sessionId: "s1", itemId: "live:100", blockIndex: 2, blockType: "text", text: " there" });
  });

  it("text deltas become session delta for the live item block", () => {
    const s = begun();
    conn(s).feed(start(200));
    conn(s).feed({ t: "delta", kind: "text", phase: "start", index: 0 });
    conn(s).feed(textDelta(0, "Check"));
    conn(s).feed(textDelta(0, "ing"));
    conn(s).feed({ t: "delta", kind: "text", phase: "end", index: 0 });
    conn(s).feed({ t: "delta", kind: "toolcall", phase: "start", index: 1 });
    conn(s).feed({ t: "delta", kind: "toolcall", phase: "delta", index: 1, delta: "{}" });
    expect(s.out).toEqual([
      { type: "session.append", sessionId: "s1", items: [{ kind: "assistant", id: "live:200", blocks: [], streaming: true }] },
      { type: "session.delta", sessionId: "s1", itemId: "live:200", blockIndex: 0, blockType: "text", text: "Check" },
      { type: "session.delta", sessionId: "s1", itemId: "live:200", blockIndex: 0, blockType: "text", text: "ing" },
    ]);
  });

  it("thinking deltas become session delta for a thinking block", () => {
    const s = begun();
    conn(s).feed(start(300, { model: "m/2" }));
    conn(s).feed({ t: "delta", kind: "thinking", phase: "start", index: 0 });
    conn(s).feed({ t: "delta", kind: "thinking", phase: "delta", index: 0, delta: "hmm" });
    conn(s).feed({ t: "delta", kind: "thinking", phase: "end", index: 0 });
    conn(s).feed({ t: "delta", kind: "text", phase: "start", index: 1 });
    conn(s).feed(textDelta(1, "ok"));
    expect(s.out.slice(1)).toEqual([
      { type: "session.delta", sessionId: "s1", itemId: "live:300", blockIndex: 0, blockType: "thinking", text: "hmm" },
      { type: "session.delta", sessionId: "s1", itemId: "live:300", blockIndex: 1, blockType: "text", text: "ok" },
    ]);
    expect((s.out[0] as { items: AssistantItem[] }).items[0]?.model).toBe("m/2");
  });

  const applyAll = (out: SessionServerMessage[]): TranscriptState => {
    let state: TranscriptState = emptyTranscript;
    for (const m of out) {
      if (m.type === "session.append") state = appendItems(state, m.items);
      if (m.type === "session.delta") state = applyDelta(state, m);
    }
    return state;
  };
  const deltasOf = (out: SessionServerMessage[]) => out.flatMap((m) => (m.type === "session.delta" ? [m] : []));
  const frameOf = (kind: string, phase: string, index: number, delta?: string) => ({ t: "delta", kind, phase, index, ...(delta === undefined ? {} : { delta }) });

  it("an unstreamed thinking block takes no block index, so the next text delta is blockIndex 0", () => {
    const s = begun();
    conn(s).feed(start(400));
    conn(s).feed(frameOf("thinking", "start", 0));
    conn(s).feed(frameOf("thinking", "end", 0));
    conn(s).feed(frameOf("text", "start", 1));
    conn(s).feed(frameOf("text", "delta", 1, "ok"));
    expect(deltasOf(s.out).map((d) => ({ blockIndex: d.blockIndex, blockType: d.blockType, text: d.text }))).toEqual([
      { blockIndex: 0, blockType: "text", text: "ok" },
    ]);
    const state = applyAll(s.out);
    expect((state.items[0] as AssistantItem).blocks).toEqual([{ type: "text", text: "ok" }]);
  });

  it("a thinking block that streams keeps blockIndex 0 and a following unstreamed block takes none", () => {
    const s = begun();
    conn(s).feed(start(500));
    conn(s).feed(frameOf("thinking", "start", 0));
    conn(s).feed(frameOf("thinking", "delta", 0, "hm"));
    conn(s).feed(frameOf("thinking", "end", 0));
    conn(s).feed(frameOf("text", "start", 1));
    conn(s).feed(frameOf("text", "end", 1));
    conn(s).feed(frameOf("text", "start", 2));
    conn(s).feed(frameOf("text", "delta", 2, "ok"));
    expect(deltasOf(s.out).map((d) => ({ blockIndex: d.blockIndex, blockType: d.blockType, text: d.text }))).toEqual([
      { blockIndex: 0, blockType: "thinking", text: "hm" },
      { blockIndex: 1, blockType: "text", text: "ok" },
    ]);
    const state = applyAll(s.out);
    expect((state.items[0] as AssistantItem).blocks).toEqual([
      { type: "thinking", text: "hm" },
      { type: "text", text: "ok" },
    ]);
  });

  const running = (callId: string, name: string, args: Record<string, unknown>, result?: { text: string; isError: boolean }): ToolItem => ({
    kind: "tool",
    id: `tool:${callId}`,
    callId,
    name,
    args,
    status: "running",
    ...(result === undefined ? {} : { result }),
  });

  it("tool start appends a running tool item", () => {
    const s = begun();
    conn(s).feed({ t: "tool_start", toolCallId: "c1", toolName: "read", args: { path: "a.md" } });
    expect(s.out).toEqual([{ type: "session.append", sessionId: "s1", items: [running("c1", "read", { path: "a.md" })] }]);
  });

  it("tool update sets the tool output", () => {
    const s = begun();
    conn(s).feed({ t: "tool_start", toolCallId: "c1", toolName: "bash", args: { command: "ls" } });
    conn(s).feed({ t: "tool_update", toolCallId: "c1", partialResult: { content: [{ type: "text", text: "a" }, { type: "text", text: "b" }] } });
    expect(s.out[1]).toEqual({
      type: "session.update",
      sessionId: "s1",
      items: [running("c1", "bash", { command: "ls" }, { text: "a\nb", isError: false })],
    });
  });

  it("tool end marks done or failed from is error", () => {
    const s = begun();
    conn(s).feed({ t: "tool_start", toolCallId: "c1", toolName: "bash", args: { command: "ls" } });
    conn(s).feed({ t: "tool_start", toolCallId: "c2", toolName: "bash", args: { command: "x" } });
    conn(s).feed({ t: "tool_end", toolCallId: "c1", result: { content: [{ type: "text", text: "fine" }] }, isError: false });
    conn(s).feed({ t: "tool_end", toolCallId: "c2", result: { content: [{ type: "text", text: "boom" }] }, isError: true });
    expect(s.out[2]).toEqual({
      type: "session.update",
      sessionId: "s1",
      items: [{ ...running("c1", "bash", { command: "ls" }, { text: "fine", isError: false }), status: "done" }],
    });
    expect(s.out[3]).toEqual({
      type: "session.update",
      sessionId: "s1",
      items: [{ ...running("c2", "bash", { command: "x" }, { text: "boom", isError: true }), status: "failed" }],
    });
  });

  it("message end finalises the live item with streaming false", () => {
    const s = begun();
    conn(s).feed(start(400));
    conn(s).feed({ t: "message_end", message: { role: "assistant", timestamp: 400, content: [{ type: "text", text: "Done." }] } });
    expect(s.out[1]).toEqual({
      type: "session.update",
      sessionId: "s1",
      items: [{ kind: "assistant", id: "live:400", blocks: [{ type: "text", text: "Done." }], streaming: false }],
    });
    conn(s).feed(start(500));
    conn(s).feed({ t: "message_end", message: { role: "assistant", timestamp: 500, content: [{ type: "toolCall", id: "c1", name: "read", arguments: {} }] } });
    expect(s.out.slice(2)).toEqual([
      { type: "session.append", sessionId: "s1", items: [{ kind: "assistant", id: "live:500", blocks: [], streaming: true }] },
      { type: "session.remove", sessionId: "s1", itemId: "live:500" },
    ]);
  });

  it("user and tool result messages are not made live", () => {
    const s = begun();
    conn(s).feed({ t: "message_start", message: { role: "user", content: "hi", timestamp: 1 } });
    conn(s).feed({ t: "message_end", message: { role: "user", content: "hi", timestamp: 1 } });
    conn(s).feed({ t: "message_start", message: { role: "toolResult", toolCallId: "c1", toolName: "read", content: [], isError: false, timestamp: 2 } });
    conn(s).feed({ t: "message_end", message: { role: "toolResult", toolCallId: "c1", toolName: "read", content: [], isError: false, timestamp: 2 } });
    conn(s).feed({ t: "agent_start" });
    conn(s).feed({ t: "agent_end" });
    expect(s.out).toEqual([]);
  });

  it("a malformed frame is ignored and later frames still apply", () => {
    const s = begun();
    conn(s).feed("not json {");
    conn(s).feed({ t: "tool_start", toolCallId: "c1", toolName: "read", args: {} });
    expect(s.out).toEqual([{ type: "session.append", sessionId: "s1", items: [running("c1", "read", {})] }]);
  });
});
