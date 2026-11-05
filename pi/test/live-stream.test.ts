import assert from "node:assert/strict";
import { Duplex } from "node:stream";
import { test } from "node:test";
import { createLiveStream, type LiveStreamDeps } from "../lib/live-stream.ts";

// docs/contract.md §6.21 (the pi live-stream socket). No real socket, timer or process: `listen` is a fake, a connection is an in-memory Duplex.

type Handler = (event: unknown, ctx?: unknown) => unknown;
type Json = Record<string, unknown>;

class Client {
  readonly chunks: string[] = [];
  readonly stream: Duplex;
  readonly received: string[] = [];
  constructor(write?: (chunk: Buffer, cb: (e?: Error | null) => void) => void) {
    this.stream = new Duplex({
      read() {},
      write: (chunk: Buffer, _enc, cb) => (write ? write(chunk, cb) : (this.chunks.push(chunk.toString()), cb())),
    });
    this.stream.on("data", (d: Buffer) => this.received.push(d.toString()));
  }
  frames(): Json[] {
    const text = this.chunks.join("");
    assert.ok(text === "" || text.endsWith("\n"), "every frame ends with a newline");
    return text === "" ? [] : text.slice(0, -1).split("\n").map((line) => JSON.parse(line) as Json);
  }
  raw(): string {
    return this.chunks.join("");
  }
}

function setup(overrides: Partial<LiveStreamDeps> = {}) {
  const calls: string[] = [];
  const stderr: string[] = [];
  const handlers = new Map<string, Handler>();
  const listens: { path: string; onConnection: (c: Duplex) => void; closed: number }[] = [];
  const deps: LiveStreamDeps = {
    listen: (path, onConnection) => {
      const entry = { path, onConnection, closed: 0 };
      listens.push(entry);
      calls.push(`listen ${path}`);
      return { close: () => { entry.closed += 1; calls.push(`close ${path}`); } };
    },
    mkdir: (path, mode) => void calls.push(`mkdir ${path} ${mode.toString(8)}`),
    unlink: (path) => void calls.push(`unlink ${path}`),
    home: "/h",
    session: "pi-7f3a",
    stderr: (text) => void stderr.push(text),
    ...overrides,
  };
  const stream = createLiveStream(deps);
  stream.attach({ on: (event, handler) => void handlers.set(event, handler as Handler) });
  const emit = (event: Json) => {
    const handler = handlers.get(event.type as string);
    assert.ok(handler, `no handler registered for ${String(event.type)}`);
    handler(event, {});
  };
  const connect = (client = new Client()) => {
    assert.equal(listens.length, 1, "listening");
    listens[0]!.onConnection(client.stream);
    return client;
  };
  return { stream, calls, stderr, handlers, listens, emit, connect };
}

const SOCK = "/h/run/pi/pi-7f3a.sock";
const start = (emit: (e: Json) => void) => emit({ type: "session_start", reason: "startup" });
const partial = { role: "assistant", content: [{ type: "text", text: "Hel" }], timestamp: 1783334400123 };
const update = (assistantMessageEvent: Json) => ({ type: "message_update", message: partial, assistantMessageEvent });

test("listens on the session socket path at session_start and creates the directory", () => {
  const s = setup();
  assert.deepEqual(s.calls, [], "nothing before session_start");
  start(s.emit);
  assert.deepEqual(s.calls, ["mkdir /h/run/pi 700", `unlink ${SOCK}`, `listen ${SOCK}`]);
});

test("removes the socket file at session_shutdown", () => {
  const s = setup();
  start(s.emit);
  s.calls.length = 0;
  s.emit({ type: "session_shutdown", reason: "quit" });
  assert.equal(s.listens[0]!.closed, 1);
  assert.ok(s.calls.includes(`unlink ${SOCK}`));
  assert.ok(s.calls.indexOf(`close ${SOCK}`) < s.calls.indexOf(`unlink ${SOCK}`));
  assert.ok(s.calls.indexOf(`close ${SOCK}`) >= 0);
});

test("unlinks a stale socket file before listening", () => {
  const s = setup();
  start(s.emit);
  assert.ok(s.calls.indexOf(`unlink ${SOCK}`) >= 0);
  assert.ok(s.calls.indexOf(`unlink ${SOCK}`) < s.calls.indexOf(`listen ${SOCK}`));
});

test("sends hello then an idle inflight frame on connect", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  assert.equal(c.raw(), '{"t":"hello","protocol":1,"session":"pi-7f3a"}\n{"t":"inflight","message":null,"tools":[]}\n');
});

test("connect during a stream sends the partial message and running tools in inflight", () => {
  const s = setup();
  start(s.emit);
  s.emit({ type: "agent_start" });
  s.emit({ type: "message_start", message: { role: "assistant", content: [], timestamp: 1783334400123 } });
  s.emit(update({ type: "text_delta", contentIndex: 0, delta: "Hel", partial }));
  s.emit({ type: "tool_execution_start", toolCallId: "t1", toolName: "read", args: { path: "a" } });
  s.emit({ type: "tool_execution_start", toolCallId: "t2", toolName: "bash", args: { command: "ls" } });
  s.emit({ type: "tool_execution_update", toolCallId: "t2", toolName: "bash", args: { command: "ls" }, partialResult: { out: "x" } });
  const c = s.connect();
  const [hello, inflight] = c.frames();
  assert.deepEqual(hello, { t: "hello", protocol: 1, session: "pi-7f3a" });
  assert.deepEqual(inflight, {
    t: "inflight",
    message: partial,
    tools: [
      { toolCallId: "t1", toolName: "read", args: { path: "a" }, partialResult: null },
      { toolCallId: "t2", toolName: "bash", args: { command: "ls" }, partialResult: { out: "x" } },
    ],
  });
  s.emit({ type: "tool_execution_end", toolCallId: "t1", toolName: "read", result: {}, isError: false });
  const later = s.connect(new Client());
  assert.deepEqual((later.frames()[1]!.tools as Json[]).map((t) => t.toolCallId), ["t2"]);
});

test("message_update becomes delta frames without repeating the partial message", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  c.chunks.length = 0;
  s.emit(update({ type: "text_delta", contentIndex: 0, delta: "lo", partial }));
  assert.equal(c.raw(), '{"t":"delta","kind":"text","phase":"delta","index":0,"delta":"lo"}\n');
  assert.ok(!c.raw().includes("message") && !c.raw().includes("partial"));
  for (const type of ["start", "done", "error"]) s.emit(update({ type, partial, message: partial, error: partial, reason: "stop" }));
  assert.equal(c.frames().length, 1, "start, done and error write no delta frame");
});

test("text thinking and toolcall deltas carry kind phase and index", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  c.chunks.length = 0;
  for (const kind of ["thinking", "text", "toolcall"]) {
    s.emit(update({ type: `${kind}_start`, contentIndex: 2, partial }));
    s.emit(update({ type: `${kind}_delta`, contentIndex: 2, delta: "d", partial }));
  }
  assert.deepEqual(c.frames(), [
    { t: "delta", kind: "thinking", phase: "start", index: 2 },
    { t: "delta", kind: "thinking", phase: "delta", index: 2, delta: "d" },
    { t: "delta", kind: "text", phase: "start", index: 2 },
    { t: "delta", kind: "text", phase: "delta", index: 2, delta: "d" },
    { t: "delta", kind: "toolcall", phase: "start", index: 2 },
    { t: "delta", kind: "toolcall", phase: "delta", index: 2, delta: "d" },
  ]);
  c.chunks.length = 0;
  s.emit(update({ type: "thinking_end", contentIndex: 0, content: "all", partial }));
  s.emit(update({ type: "text_end", contentIndex: 1, content: "all", partial }));
  assert.deepEqual(c.frames(), [
    { t: "delta", kind: "thinking", phase: "end", index: 0 },
    { t: "delta", kind: "text", phase: "end", index: 1 },
  ]);
});

test("toolcall end carries the finished tool call", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  c.chunks.length = 0;
  const toolCall = { type: "toolCall", id: "toolu_01", name: "read", arguments: { path: "README.md" } };
  s.emit(update({ type: "toolcall_end", contentIndex: 1, toolCall, partial }));
  assert.equal(c.raw(), `${JSON.stringify({ t: "delta", kind: "toolcall", phase: "end", index: 1, toolCall })}\n`);
});

test("tool execution start update end become tool frames", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  c.chunks.length = 0;
  s.emit({ type: "tool_execution_start", toolCallId: "toolu_01", toolName: "read", args: { path: "README.md" }, parentToolCallId: "p" });
  s.emit({ type: "tool_execution_update", toolCallId: "toolu_01", toolName: "read", args: { path: "README.md" }, partialResult: { n: 1 } });
  s.emit({ type: "tool_execution_end", toolCallId: "toolu_01", toolName: "read", result: { n: 2 }, isError: true, durationMs: 5 });
  assert.equal(
    c.raw(),
    '{"t":"tool_start","toolCallId":"toolu_01","toolName":"read","args":{"path":"README.md"}}\n' +
      '{"t":"tool_update","toolCallId":"toolu_01","partialResult":{"n":1}}\n' +
      '{"t":"tool_end","toolCallId":"toolu_01","result":{"n":2},"isError":true}\n',
  );
});

test("agent and message lifecycle frames are forwarded", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  c.chunks.length = 0;
  const user = { role: "user", content: "hi", timestamp: 1 };
  s.emit({ type: "agent_start" });
  s.emit({ type: "message_start", message: user });
  s.emit({ type: "message_end", message: user });
  s.emit({ type: "agent_end", messages: [user] });
  assert.deepEqual(c.frames(), [{ t: "agent_start" }, { t: "message_start", message: user }, { t: "message_end", message: user }, { t: "agent_end" }]);
});

test("a failed listen reports once on stderr and never throws", () => {
  const s = setup({ listen: () => { throw new Error("EADDRINUSE"); } });
  assert.doesNotThrow(() => start(s.emit));
  assert.equal(s.stderr.length, 1);
  assert.match(s.stderr[0]!, /EADDRINUSE/);
  assert.doesNotThrow(() => {
    s.emit({ type: "agent_start" });
    s.emit({ type: "message_start", message: partial });
    s.emit(update({ type: "text_delta", contentIndex: 0, delta: "x", partial }));
    s.emit({ type: "session_shutdown", reason: "quit" });
  });
  assert.equal(s.stderr.length, 1, "reported once");
});

test("a client over 1 MiB pending is disconnected and others still receive frames", () => {
  const s = setup();
  start(s.emit);
  const slow = s.connect(new Client(() => { /* never completes: the bytes stay pending */ }));
  const fast = s.connect();
  fast.chunks.length = 0;
  const big = "x".repeat(600 * 1024);
  s.emit({ type: "message_start", message: { role: "user", content: big } });
  s.emit({ type: "message_start", message: { role: "user", content: big } });
  assert.equal(slow.stream.destroyed, true, "the slow client is disconnected");
  assert.equal(fast.stream.destroyed, false);
  assert.equal(fast.frames().length, 2);
  s.emit({ type: "agent_end", messages: [] });
  assert.deepEqual(fast.frames().at(-1), { t: "agent_end" });
});

test("a throwing client write never throws into pi", () => {
  const s = setup();
  start(s.emit);
  const bad = s.connect();
  bad.stream.write = (() => { throw new Error("EPIPE"); }) as never;
  const good = s.connect();
  good.chunks.length = 0;
  assert.doesNotThrow(() => s.emit({ type: "agent_start" }));
  assert.doesNotThrow(() => s.emit({ type: "agent_end", messages: [] }));
  assert.deepEqual(good.frames(), [{ t: "agent_start" }, { t: "agent_end" }]);
});

test("client input is ignored", () => {
  const s = setup();
  start(s.emit);
  const c = s.connect();
  c.chunks.length = 0;
  assert.doesNotThrow(() => {
    c.stream.push(Buffer.from('{"t":"hello","protocol":1}\n{"t":"abort"}\nnot json\n'));
  });
  assert.equal(c.raw(), "", "nothing is written in answer");
  s.emit({ type: "agent_start" });
  assert.deepEqual(c.frames(), [{ t: "agent_start" }]);
  assert.equal(c.stream.destroyed, false);
});
