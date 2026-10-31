import assert from "node:assert/strict";
import { test } from "node:test";
import { LIMITS, ProtocolError } from "../../durable-host/protocol.ts";
import { Ring } from "../../durable-host/ring.ts";
import { type Connection, createServer, type ServerDeps } from "../../durable-host/server.ts";

const HELLO = JSON.stringify({ type: "hello", protocol: 1, min_protocol: 1, client: "t" });
const snap = { type: "snapshot", n: 0 };

type FakeConn = Connection & { lines: string[]; closes: number; buf: number; msgs(): any[] };
const conn = (): FakeConn => {
  const c: FakeConn = {
    lines: [],
    closes: 0,
    buf: 0,
    write(l: string) {
      c.lines.push(l);
    },
    buffered: () => c.buf,
    close() {
      c.closes++;
    },
    msgs: () => c.lines.map((l) => JSON.parse(l)),
  };
  return c;
};

type Timer = { ms: number; fn: () => void; cancelled: boolean };

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

function setup(over: Partial<ServerDeps> = {}) {
  const timers: Timer[] = [];
  let n = 0;
  const calls = { submit: [] as any[], abort: [] as any[], abortSubmission: [] as any[], answerUi: [] as any[] };
  const ring = new Ring("e1", snap);
  const state = { current: { state: "idle" as const, held_ui: 0 } as any };
  const deps: ServerDeps = {
    session: "s1",
    role: "lead",
    cwd: "/w",
    hostVersion: "9.9",
    piDurable: "1.3.0",
    epoch: "e1",
    ring,
    submit: async (i) => {
      calls.submit.push(i);
      return { submissionId: 7, status: "accepted" };
    },
    abort: async (o) => {
      calls.abort.push(o);
    },
    abortSubmission: async (id) => {
      calls.abortSubmission.push(id);
      return "aborted";
    },
    hostState: () => state.current,
    held: () => [],
    answerUi: async (id, value) => {
      calls.answerUi.push({ id, value });
      return { outcome: "answered" };
    },
    timers: {
      after(ms, fn) {
        const t: Timer = { ms, fn, cancelled: false };
        timers.push(t);
        return () => {
          t.cancelled = true;
        };
      },
    },
    clientId: () => `c${++n}`,
    ...over,
  };
  const server = createServer(deps);
  const open = (greet = true) => {
    const c = conn();
    const h = server.connect(c);
    if (greet) h.receive(HELLO);
    return { c, h };
  };
  const attached = (cursor?: object) => {
    const x = open();
    x.h.receive(JSON.stringify({ type: "attach", req: "a", ...(cursor ? { cursor } : {}) }));
    return x;
  };
  return { server, deps, ring, timers, calls, state, open, attached };
}

const hostState = (clients: number) => ({ type: "host_state", state: "idle", clients, held_ui: 0 });

test("hello is answered by welcome with the picked protocol, the epoch and a client id", () => {
  const { open } = setup();
  const { c } = open();
  assert.deepEqual(c.msgs(), [
    {
      type: "welcome",
      protocol: 1,
      host_version: "9.9",
      pi_durable: "1.3.0",
      session: "s1",
      role: "lead",
      epoch: "e1",
      client_id: "c1",
      cwd: "/w",
      limits: { client_line: LIMITS.clientLine },
    },
  ]);
  assert.equal(c.closes, 0);
});

test("a hello with no common protocol gets protocol_mismatch with the versions and the connection is closed", () => {
  const { open } = setup();
  const { c, h } = open(false);
  h.receive(JSON.stringify({ type: "hello", protocol: 3, min_protocol: 2, client: "t" }));
  const [m] = c.msgs();
  assert.equal(m.type, "error");
  assert.equal(m.code, "protocol_mismatch");
  assert.deepEqual(m.details, { host: [1], client: [2, 3] });
  assert.equal(c.msgs().length, 1);
  assert.equal(c.closes, 1);
});

test("a message before hello gets hello_required and the connection is kept", () => {
  const { open, timers } = setup();
  const { c, h } = open(false);
  h.receive(JSON.stringify({ type: "submit", req: "r1", text: "x" }));
  const msgs = c.msgs();
  assert.equal(msgs.length, 1);
  assert.equal(msgs[0].type, "error");
  assert.equal(msgs[0].code, "hello_required");
  assert.equal(msgs[0].req, "r1");
  assert.equal(c.closes, 0);
  assert.equal(timers[0].cancelled, false);
});

test("no hello within the hello timeout gets hello_timeout and the connection is closed", () => {
  const { open, timers } = setup();
  const { c } = open(false);
  assert.equal(timers.length, 1);
  assert.equal(timers[0].ms, LIMITS.helloMs);
  assert.deepEqual(c.lines, []);
  timers[0].fn();
  const msgs = c.msgs();
  assert.equal(msgs.length, 1);
  assert.equal(msgs[0].type, "error");
  assert.equal(msgs[0].code, "hello_timeout");
  assert.equal(c.closes, 1);
});

test("a hello cancels the hello timer", () => {
  const { open, timers } = setup();
  open();
  assert.equal(timers.length, 1);
  assert.equal(timers[0].cancelled, true);
});

test("submit, abort and abort_submission before attach get not_attached", () => {
  const { open, calls } = setup();
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "submit", req: "r1", text: "x" }));
  h.receive(JSON.stringify({ type: "abort", req: "r2" }));
  h.receive(JSON.stringify({ type: "abort_submission", req: "r3", submission_id: 1 }));
  const errs = c.msgs().slice(1);
  assert.deepEqual(
    errs.map((m) => [m.type, m.code, m.req]),
    [
      ["error", "not_attached", "r1"],
      ["error", "not_attached", "r2"],
      ["error", "not_attached", "r3"],
    ],
  );
  assert.equal(c.closes, 0);
  assert.deepEqual([calls.submit, calls.abort, calls.abortSubmission], [[], [], []]);
});

test("ping works before attach and carries its req", () => {
  const { open } = setup();
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "ping", req: "p1" }));
  h.receive(JSON.stringify({ type: "ping" }));
  assert.deepEqual(c.msgs().slice(1), [{ type: "pong", req: "p1" }, { type: "pong" }]);
  const d = open(false);
  d.h.receive(JSON.stringify({ type: "ping", req: "p2" }));
  assert.deepEqual(d.c.msgs(), [{ type: "pong", req: "p2" }]);
});

test("attach without a cursor answers attached in snapshot mode then the snapshot frame, the ring frames and host_state", () => {
  const { open, ring, server } = setup();
  const f2 = ring.append([{ type: "message", n: 1 }]);
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "attach", req: "a1" }));
  assert.deepEqual(c.msgs().slice(1), [
    { type: "attached", req: "a1", mode: "snapshot", epoch: "e1", from_seq: 1, head_seq: 2, client_id: "c1" },
    { type: "snapshot", epoch: "e1", seq: 1, snapshot: snap },
    f2,
    hostState(1),
  ]);
  assert.equal(server.clients(), 1);
});

test("attach with a cursor in the ring answers attached in resume mode with only the missed frames", () => {
  const { open, ring } = setup();
  ring.append([{ n: 1 }]);
  const f3 = ring.append([{ n: 2 }]);
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "attach", req: "a1", cursor: { epoch: "e1", seq: 2 } }));
  assert.deepEqual(c.msgs().slice(1), [
    { type: "attached", req: "a1", mode: "resume", epoch: "e1", from_seq: 3, head_seq: 3, client_id: "c1" },
    f3,
    hostState(1),
  ]);
});

test("attach with a conversation other than root gets unsupported", () => {
  const { open, server } = setup();
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "attach", req: "a1", conversation: "other" }));
  const msgs = c.msgs().slice(1);
  assert.equal(msgs.length, 1);
  assert.equal(msgs[0].type, "error");
  assert.equal(msgs[0].code, "unsupported");
  assert.equal(msgs[0].req, "a1");
  assert.equal(server.clients(), 0);
  h.receive(JSON.stringify({ type: "attach", req: "a2", conversation: "root" }));
  assert.equal(c.msgs()[2].type, "attached");
});

test("a live frame goes to every attached client and not to a client that has not attached", () => {
  const { open, attached, ring, server } = setup();
  const a = attached();
  const b = attached();
  const idle = open();
  const before = [a.c.lines.length, b.c.lines.length, idle.c.lines.length];
  const f = ring.append([{ n: 1 }]);
  server.frame(f);
  assert.deepEqual(a.c.msgs().slice(before[0]), [f]);
  assert.deepEqual(b.c.msgs().slice(before[1]), [f]);
  assert.equal(idle.c.lines.length, before[2]);
});

test("a client attaching while frames arrive gets every frame once and in seq order", () => {
  const { attached, ring, server } = setup();
  attached();
  const f2 = ring.append([{ n: 2 }]);
  server.frame(f2);
  const late = attached();
  const f3 = ring.append([{ n: 3 }]);
  server.frame(f3);
  const f4 = ring.append([{ n: 4 }]);
  server.frame(f4);
  const seqs = late.c
    .msgs()
    .filter((m) => m.type === "snapshot" || m.type === "events")
    .map((m) => m.seq);
  assert.deepEqual(seqs, [1, 2, 3, 4]);
});

test("submit calls the port with the text, when_busy and request_id and answers result with submission_id and status", async () => {
  const { attached, calls } = setup();
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "submit", req: "s1", text: "hi", when_busy: "steer", request_id: "q1" }));
  await flush();
  assert.deepEqual(calls.submit, [{ text: "hi", whenBusy: "steer", requestId: "q1" }]);
  assert.deepEqual(c.msgs().slice(n), [{ type: "result", req: "s1", data: { submission_id: 7, status: "accepted" } }]);
});

test("submit defaults when_busy to followUp", async () => {
  const { attached, calls } = setup();
  const { h } = attached();
  h.receive(JSON.stringify({ type: "submit", req: "s1", text: "hi" }));
  await flush();
  assert.equal(calls.submit.length, 1);
  assert.equal(calls.submit[0].text, "hi");
  assert.equal(calls.submit[0].whenBusy, "followUp");
});

test("a port that throws a busy ProtocolError gets error busy with the req", async () => {
  const { attached } = setup({
    submit: async () => {
      throw new ProtocolError("busy", "agent is busy", { why: "x" });
    },
  });
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "submit", req: "s1", text: "hi", when_busy: "reject" }));
  await flush();
  assert.deepEqual(c.msgs().slice(n), [{ type: "error", req: "s1", code: "busy", message: "agent is busy", details: { why: "x" } }]);
  assert.equal(c.closes, 0);
});

test("abort calls the port with background and answers an empty result", async () => {
  const { attached, calls } = setup();
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "abort", req: "b1", background: true }));
  await flush();
  assert.deepEqual(calls.abort, [{ background: true }]);
  assert.deepEqual(c.msgs().slice(n), [{ type: "result", req: "b1", data: {} }]);
});

test("abort_submission answers result with the outcome of the port", async () => {
  const { attached } = setup({ abortSubmission: async () => "settled" });
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "abort_submission", req: "b1", submission_id: 5 }));
  await flush();
  assert.deepEqual(c.msgs().slice(n), [{ type: "result", req: "b1", data: { outcome: "settled" } }]);
});

test("detach answers result then bye detached and closes the connection", () => {
  const { attached, server } = setup();
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "detach", req: "d1" }));
  assert.deepEqual(c.msgs().slice(n), [
    { type: "result", req: "d1", data: {} },
    { type: "bye", reason: "detached", reconnect: false },
  ]);
  assert.equal(c.closes, 1);
  assert.equal(server.clients(), 0);
});

test("a port that throws another error gets error internal and the connection is kept", async () => {
  const { attached } = setup({
    abort: async () => {
      throw new Error("boom");
    },
  });
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "abort", req: "b1" }));
  await flush();
  const msgs = c.msgs().slice(n);
  assert.equal(msgs.length, 1);
  assert.equal(msgs[0].type, "error");
  assert.equal(msgs[0].code, "internal");
  assert.equal(msgs[0].req, "b1");
  assert.equal(c.closes, 0);
});

test("an unknown type gets unknown_type and the connection is kept", () => {
  const { open } = setup();
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "compact", req: "k1" }));
  const m = c.msgs()[1];
  assert.equal(m.type, "error");
  assert.equal(m.code, "unknown_type");
  assert.equal(m.req, "k1");
  assert.equal(c.closes, 0);
  h.receive(JSON.stringify({ type: "ping", req: "p" }));
  assert.deepEqual(c.msgs()[2], { type: "pong", req: "p" });
});

test("a line over the client line limit gets frame_too_large and the connection is closed", () => {
  const { open } = setup();
  const { c, h } = open();
  h.receive("x".repeat(LIMITS.clientLine + 1));
  const msgs = c.msgs().slice(1);
  assert.equal(msgs.length, 1);
  assert.equal(msgs[0].type, "error");
  assert.equal(msgs[0].code, "frame_too_large");
  assert.equal(c.closes, 1);
});

test("a client whose output exceeds the slow client limit gets bye slow_client and is closed", () => {
  const { attached, ring, server } = setup();
  const { c } = attached();
  c.buf = LIMITS.slowClient + 1;
  const n = c.lines.length;
  server.frame(ring.append([{ n: 1 }]));
  const msgs = c.msgs().slice(n);
  assert.equal(msgs[0].type, "events");
  assert.deepEqual(msgs[msgs.length - 1], { type: "bye", reason: "slow_client", reconnect: true });
  assert.equal(c.closes, 1);
});

test("stateChanged writes host_state with clients and held_ui 0 to attached clients", () => {
  const { open, attached, server, state } = setup();
  const a = attached();
  const b = attached();
  const idle = open();
  const na = a.c.lines.length;
  const ni = idle.c.lines.length;
  state.current = { state: "working", name: "n", held_ui: 0 };
  server.stateChanged();
  const want = { type: "host_state", state: "working", name: "n", clients: 2, held_ui: 0 };
  assert.deepEqual(a.c.msgs().slice(na), [want]);
  assert.deepEqual(b.c.msgs().slice(-1), [want]);
  assert.equal(idle.c.lines.length, ni);
});

test("clients counts attached connections and forgets a closed one", () => {
  const { open, attached, server, timers } = setup();
  assert.equal(server.clients(), 0);
  const a = attached();
  const b = attached();
  open();
  assert.equal(server.clients(), 2);
  a.h.closed();
  assert.equal(server.clients(), 1);
  b.h.closed();
  assert.equal(server.clients(), 0);
  const u = open(false);
  u.h.closed();
  assert.equal(timers[timers.length - 1].cancelled, true);
});

test("shutdown writes bye with reconnect true to every client and closes them", () => {
  const { open, attached, server, timers } = setup();
  const a = attached();
  const idle = open();
  const ungreeted = open(false);
  server.shutdown("restart");
  const bye = { type: "bye", reason: "restart", reconnect: true };
  for (const x of [a.c, idle.c, ungreeted.c]) {
    assert.deepEqual(x.msgs().slice(-1), [bye]);
    assert.equal(x.closes, 1);
  }
  assert.ok(timers.every((t) => t.cancelled));
});

test("a connection made after shutdown gets shutting_down and is closed", () => {
  const { server } = setup();
  server.shutdown("shutdown");
  const c = conn();
  server.connect(c);
  const msgs = c.msgs();
  assert.equal(msgs.length, 1);
  assert.equal(msgs[0].type, "error");
  assert.equal(msgs[0].code, "shutting_down");
  assert.equal(c.closes, 1);
});

test("ui_response before attach gets not_attached", () => {
  const { open, calls } = setup();
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "ui_response", req: "u0", id: "i1", value: "Allow once" }));
  assert.deepEqual(c.msgs().slice(1), [{ type: "error", req: "u0", code: "not_attached", message: "attach first" }]);
  assert.equal(c.closes, 0);
  assert.deepEqual(calls.answerUi, []);
});

test("ui_response for a held id answers result then broadcasts ui_resolved answered to every attached client with the sender as by", async () => {
  const { attached, calls } = setup({
    held: () => [{ id: "u1", method: "select", purpose: "secret_approve", created_at: 100, fields: { title: "approve", options: ["Allow once", "Deny"] } }],
  });
  const a = attached();
  const b = attached();
  const na = a.c.lines.length;
  const nb = b.c.lines.length;
  a.h.receive(JSON.stringify({ type: "ui_response", req: "r1", id: "u1", value: "Allow once" }));
  await flush();
  assert.deepEqual(calls.answerUi, [{ id: "u1", value: { value: "Allow once", confirmed: undefined, answer: undefined, cancelled: undefined } }]);
  assert.deepEqual(a.c.msgs().slice(na), [
    { type: "result", req: "r1", data: {} },
    { type: "ui_resolved", id: "u1", outcome: "answered", by: "c1" },
  ]);
  assert.deepEqual(b.c.msgs().slice(nb), [{ type: "ui_resolved", id: "u1", outcome: "answered", by: "c1" }]);
});

test("ui_response for a value the host answers cancelled broadcasts ui_resolved cancelled", async () => {
  const { attached } = setup({
    answerUi: async (id, value) => {
      assert.equal(value.cancelled, true);
      return { outcome: "cancelled" };
    },
  });
  const a = attached();
  const b = attached();
  const na = a.c.lines.length;
  const nb = b.c.lines.length;
  a.h.receive(JSON.stringify({ type: "ui_response", req: "r1", id: "u1", cancelled: true }));
  await flush();
  assert.deepEqual(a.c.msgs().slice(na), [
    { type: "result", req: "r1", data: {} },
    { type: "ui_resolved", id: "u1", outcome: "cancelled", by: "c1" },
  ]);
  assert.deepEqual(b.c.msgs().slice(nb), [{ type: "ui_resolved", id: "u1", outcome: "cancelled", by: "c1" }]);
});

test("ui_response for an id not held gets ui_unknown and the connection is kept", async () => {
  const { attached, calls } = setup({
    answerUi: async (id, value) => {
      calls.answerUi.push({ id, value });
      throw new ProtocolError("ui_unknown", `no held request ${id}`);
    },
  });
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "ui_response", req: "r1", id: "gone", value: "Allow once" }));
  await flush();
  assert.deepEqual(c.msgs().slice(n), [{ type: "error", req: "r1", code: "ui_unknown", message: "no held request gone" }]);
  assert.equal(c.closes, 0);
  assert.equal(calls.answerUi.length, 1);
  h.receive(JSON.stringify({ type: "ping", req: "p" }));
  assert.deepEqual(c.msgs().slice(-1), [{ type: "pong", req: "p" }]);
});

test("attach replays each held request with replayed true after the frames and before host_state", () => {
  const held = [
    { id: "u1", method: "select" as const, purpose: "secret_approve" as const, created_at: 100, fields: { title: "approve", options: ["Allow once", "Deny"] } },
    { id: "u2", method: "input" as const, purpose: "ask_user_question" as const, created_at: 200, expires_at: 500, fields: { title: "name", placeholder: "your name" } },
  ];
  const { open } = setup({ held: () => held });
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "attach", req: "a1" }));
  assert.deepEqual(c.msgs().slice(1), [
    { type: "attached", req: "a1", mode: "snapshot", epoch: "e1", from_seq: 1, head_seq: 1, client_id: "c1" },
    { type: "snapshot", epoch: "e1", seq: 1, snapshot: snap },
    { type: "ui_request", id: "u1", method: "select", title: "approve", options: ["Allow once", "Deny"], purpose: "secret_approve", created_at: 100, replayed: true },
    { type: "ui_request", id: "u2", method: "input", title: "name", placeholder: "your name", purpose: "ask_user_question", created_at: 200, expires_at: 500, replayed: true },
    hostState(1),
  ]);
});

test("attach replays held requests in order of creation and does not change their ids", () => {
  const held = [
    { id: "u1", method: "select" as const, purpose: "other" as const, created_at: 100, fields: { title: "a", options: [] } },
    { id: "u2", method: "select" as const, purpose: "other" as const, created_at: 200, fields: { title: "b", options: [] } },
    { id: "u3", method: "confirm" as const, purpose: "plan_review" as const, created_at: 300, fields: { title: "c", message: "m" } },
  ];
  const { open } = setup({ held: () => held });
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "attach", req: "a1" }));
  const requests = c.msgs().filter((m) => m.type === "ui_request");
  assert.deepEqual(requests.map((m) => m.id), ["u1", "u2", "u3"]);
  assert.ok(requests.every((m) => m.replayed === true));
  assert.deepEqual(requests.map((m) => m.title), ["a", "b", "c"]);
});

test("attach with no held request sends no ui_request", () => {
  const { open } = setup();
  const { c, h } = open();
  h.receive(JSON.stringify({ type: "attach", req: "a1" }));
  assert.deepEqual(
    c.msgs().slice(1).map((m) => m.type),
    ["attached", "snapshot", "host_state"],
  );
});

test("host_state carries the held_ui of the deps and not a server constant", () => {
  const { open, attached, server, state } = setup();
  state.current = { state: "blocked", name: "n", held_ui: 2 };
  const a = attached();
  const idle = open();
  const na = a.c.lines.length;
  assert.deepEqual(a.c.msgs().slice(-1), [{ type: "host_state", state: "blocked", name: "n", clients: 1, held_ui: 2 }]);
  state.current = { state: "blocked", name: "n", held_ui: 3 };
  server.stateChanged();
  assert.deepEqual(a.c.msgs().slice(na), [{ type: "host_state", state: "blocked", name: "n", clients: 1, held_ui: 3 }]);
  assert.equal(idle.c.lines.length, 1);
});

test("broadcast sends a ui_request, notice and status to every attached client and to none that is not attached", () => {
  const { open, attached, server } = setup();
  const a = attached();
  const b = attached();
  const idle = open();
  const na = a.c.lines.length;
  const nb = b.c.lines.length;
  const ni = idle.c.lines.length;
  server.broadcast({ type: "ui_request", id: "u1", method: "select", title: "t", options: ["x"], purpose: "other", created_at: 1 });
  server.broadcast({ type: "notice", level: "info", message: "hi" });
  server.broadcast({ type: "status", key: "cpd", text: "working" });
  server.broadcast({ type: "status", key: "cpd", text: null });
  const want = [
    { type: "ui_request", id: "u1", method: "select", title: "t", options: ["x"], purpose: "other", created_at: 1 },
    { type: "notice", level: "info", message: "hi" },
    { type: "status", key: "cpd", text: "working" },
    { type: "status", key: "cpd", text: null },
  ];
  assert.deepEqual(a.c.msgs().slice(na), want);
  assert.deepEqual(b.c.msgs().slice(nb), want);
  assert.equal(idle.c.lines.length, ni);
});

test("a late ui_response after the resolver changed nothing answers ui_unknown once only", async () => {
  const resolved = new Set<string>();
  const { attached, calls } = setup({
    answerUi: async (id, value) => {
      calls.answerUi.push({ id, value });
      if (resolved.has(id)) throw new ProtocolError("ui_unknown", `no held request ${id}`);
      resolved.add(id);
      return { outcome: "answered" };
    },
  });
  const { c, h } = attached();
  const n = c.lines.length;
  h.receive(JSON.stringify({ type: "ui_response", req: "r1", id: "u1", value: "Allow once" }));
  await flush();
  assert.deepEqual(c.msgs().slice(n), [
    { type: "result", req: "r1", data: {} },
    { type: "ui_resolved", id: "u1", outcome: "answered", by: "c1" },
  ]);
  const m = c.lines.length;
  h.receive(JSON.stringify({ type: "ui_response", req: "r2", id: "u1", value: "Deny" }));
  await flush();
  // The host keeps no resolved ids (Q16): the late answer is one ui_unknown, no result, no broadcast, the connection kept.
  assert.deepEqual(c.msgs().slice(m), [{ type: "error", req: "r2", code: "ui_unknown", message: "no held request u1" }]);
  assert.equal(c.closes, 0);
  assert.deepEqual(calls.answerUi.map((a) => a.id), ["u1", "u1"]);
});
