import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import { LIMITS } from "../../durable-host/protocol.ts";
import { CONFIG, EPOCH, withHost } from "./rig.ts";

const SOCK = "/cpd-home/run/host/s-lead.sock";
const IDLE = ["session", "state", "idle", "--session", CONFIG.session];
const IDLE_REPLY = ["session", "state", "idle", "--reply", "--session", CONFIG.session];
const viewFrame = (seq: number, n: number) => ({ type: "view", epoch: EPOCH, seq, view: { type: "view", n } });
const opsFrame = (seq: number, ops: unknown[]) => ({ type: "ops", epoch: EPOCH, seq, ops });
const live = (run: boolean) => ({ docs: { "pi.live": run ? { run: { id: 1 } } : {} } });
const submit = (req: string, over: Record<string, unknown> = {}) => ({ type: "submit", req, text: "hi", when_busy: "followUp", ...over });
const types = (c: { received(): Record<string, unknown>[] }) => c.received().map((m) => m.type);
const hostStates = (c: { received(): Record<string, unknown>[] }) => c.received().filter((m) => m.type === "host_state");
/** A context update is stored before an input that starts a turn when cpd has news. */
const NEWS = { "events-list": { ok: true, events: [] }, "board-take": { ok: true, taken: [{ at: "2026-10-14T21:00:00Z", from_session: null, from_name: null, text: "Mind the schema." }] } };

test("runHost refuses a socket path over the limit with a usage error before it opens the store", { timeout: 20_000 }, async () => {
  const home = `/${"h".repeat(110)}`;
  await withHost({ config: { ...CONFIG, home } }, async (r) => {
    await r.ready();
    assert.ok(r.ending.error instanceof CpdError, `the host did not fail: ${String(r.ending.error)}`);
    assert.equal(r.ending.error.code, "usage");
    assert.match(r.ending.error.message, /socket_path_too_long/);
    assert.equal(r.opened.n, 0);
    assert.equal(r.listens.length, 0);
    assert.deepEqual(r.calls, [], "before any effect");
  });
});

test("runHost listens on the socket path of the Session after it opens the store", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    assert.deepEqual(r.listens.map((l) => l.path), [SOCK]);
    assert.deepEqual(r.order.slice(0, 3), ["guard", "open", "watch"]);
  });
});

test("runHost gives an attaching client the view of the watch as the first frame", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    const got = c.received();
    assert.equal(got[0]?.type, "welcome");
    assert.deepEqual(got[1], { type: "attached", req: "attach-1", mode: "view", epoch: EPOCH, from_seq: 1, head_seq: 1, client_id: "c1" });
    assert.deepEqual(got[2], viewFrame(1, 1));
    assert.equal(got[3]?.type, "host_state");
  });
});

test("runHost appends the ops of each watch call to the ring and writes them to attached clients", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const first = await r.attach();
    const o1 = ["p", ["entries"], 0, 0, [{ id: "a" }]];
    const o2 = ["s", ["docs", "pi.live"], { run: null }];
    const o3 = ["a", ["docs", "pi.live", "text"], "x"];
    await r.streams[0]!.push([o1, o2], { n: "v2" });
    await r.streams[0]!.push([o3], { n: "v3" });
    assert.deepEqual(first.received().filter((m) => m.type === "ops"), [opsFrame(2, [o1, o2]), opsFrame(3, [o3])]);

    const late = await r.attach();
    assert.deepEqual(late.received().slice(1, 5).map((m) => m.type), ["attached", "view", "ops", "ops"]);
    assert.equal(late.received()[1]?.head_seq, 3);

    const resumed = await r.attach({ epoch: EPOCH, seq: 2 });
    assert.deepEqual(resumed.received()[1], { type: "attached", req: "attach-1", mode: "resume", epoch: EPOCH, from_seq: 3, head_seq: 3, client_id: "c3" });
    assert.deepEqual(resumed.received()[2], opsFrame(3, [o3]));
  });
});

test("runHost rebases the ring with the current view at a run end without opening the watch again", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    const op = ["a", ["docs", "pi.live", "text"], "x"];
    await r.streams[0]!.push([], live(true));
    for (let i = 1; i < LIMITS.rebaseAtRunEnd; i++) await r.streams[0]!.push([op], live(true));
    assert.equal(r.streams[0]!.stopped, 0, "no rebase below the bound, run end or not");
    const ended = live(false);
    await r.streams[0]!.push([["d", ["docs", "pi.live"]]], ended);
    assert.equal(r.streams.length, 1, "the watch is not opened again");
    assert.equal(r.streams[0]!.stopped, 0, "the watch is not stopped");
    const got = c.received();
    assert.deepEqual(got.at(-2), opsFrame(LIMITS.rebaseAtRunEnd + 2, [["d", ["docs", "pi.live"]]]));
    assert.deepEqual(got.at(-1), { type: "view", epoch: EPOCH, seq: LIMITS.rebaseAtRunEnd + 3, view: ended });
    assert.equal(r.ending.code, undefined, "a rebase is not a failure");
    assert.ok(!got.some((m) => m.type === "bye"));

    const late = await r.attach();
    assert.deepEqual(late.received()[1], { type: "attached", req: "attach-1", mode: "view", epoch: EPOCH, from_seq: LIMITS.rebaseAtRunEnd + 3, head_seq: LIMITS.rebaseAtRunEnd + 3, client_id: "c2" });
  });
});

test("runHost submits a client message as chat:<request_id> with a cpd-chat entry", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    c.send(submit("r1", { request_id: "q1" }));
    await r.drain();
    assert.equal(r.submitted.length, 1);
    const sent = r.submitted[0]!;
    assert.equal(sent.content, "hi");
    assert.equal(sent.requestId, "chat:q1");
    assert.equal(sent.whenBusy, "followUp");
    assert.deepEqual(sent.entry, { kind: "cpd-chat", data: { client_id: "c1", request_id: "q1" } });
    assert.deepEqual(c.received().filter((m) => m.type === "result"), [{ type: "result", req: "r1", data: { submission_id: 41, status: "queued" } }]);

    c.send(submit("r2", { when_busy: "steer" }));
    await r.drain();
    const generated = r.submitted[1]!;
    assert.equal(generated.whenBusy, "steer");
    const id = (generated.entry.data as { request_id: string }).request_id;
    assert.ok(id.length > 0, "a request id was generated");
    assert.equal(generated.requestId, `chat:${id}`);
    assert.equal(generated.entry.kind, "cpd-chat");
    assert.equal((generated.entry.data as { client_id: string }).client_id, "c1");
  });
});

test("runHost makes a client message a reply run and reports working then idle with reply", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    c.send(submit("r1"));
    await r.drain();
    assert.deepEqual(r.states(), ["working"]);
    r.finish[0]!("done");
    await r.drain();
    assert.deepEqual(r.states(), ["working", "idle"]);
    assert.deepEqual(r.stateCalls().at(-1), IDLE_REPLY);
    assert.notDeepEqual(r.stateCalls().at(-1), IDLE);
    assert.equal(r.submitted[0]!.reply, true);
  });
});

test("runHost gives a client message that starts a turn the context update first", { timeout: 20_000 }, async () => {
  await withHost({ answers: NEWS }, async (r) => {
    await r.ready();
    const c = await r.attach();
    c.send(submit("r1", { request_id: "q1" }));
    await r.drain();
    const from = r.order.indexOf("write:cpd-context");
    assert.ok(from >= 0, `no context update in ${r.order.join(", ")}`);
    assert.equal(r.order.indexOf("submit:chat:q1"), from + 1, "the update comes right before the input");

    const steered = r.submitted.length;
    c.send(submit("r2", { request_id: "q2", when_busy: "steer" }));
    await r.drain();
    assert.equal(r.submitted.length, steered + 1);
    assert.equal(r.order.filter((o) => o === "write:cpd-context").length, 1, "an input joining a running turn gets no second update");
  });
});

test("runHost answers a client submit with when_busy reject while the conversation is busy with error busy", { timeout: 20_000 }, async () => {
  const busy = { value: true };
  await withHost({ busy, answers: NEWS }, async (r) => {
    await r.ready();
    const c = await r.attach();
    c.send(submit("r1", { when_busy: "reject" }));
    await r.drain();
    const errors = c.received().filter((m) => m.type === "error");
    assert.equal(errors.length, 1);
    assert.equal(errors[0]!.req, "r1");
    assert.equal(errors[0]!.code, "busy");
    assert.equal(typeof errors[0]!.message, "string");
    assert.equal(c.closed, 0, "the connection is kept");
    // P1.9 writes the skills baseline at host start; the rejected submit itself stores nothing.
    assert.deepEqual(r.written.filter((w) => (w as { entry?: { kind?: string } }).entry?.kind !== "cpd-skills"), [], "a refused reject stores nothing");

    busy.value = false;
    c.send(submit("r2", { when_busy: "reject" }));
    await r.drain();
    assert.equal(r.submitted.at(-1)?.whenBusy, "reject", "reject reaches the conversation");
    assert.deepEqual(c.received().filter((m) => m.type === "result").map((m) => m.req), ["r2"]);
  });
});

test("runHost passes abort and abort_submission of a client to the conversation", { timeout: 20_000 }, async () => {
  await withHost({ abortSubmission: "already_placed" }, async (r) => {
    await r.ready();
    const c = await r.attach();
    c.send({ type: "abort", req: "a1", background: true });
    c.send({ type: "abort_submission", req: "a2", submission_id: 9 });
    await r.drain();
    assert.deepEqual(r.aborts, [{ background: true }]);
    assert.deepEqual(r.abortedSubmissions, [9]);
    assert.deepEqual(c.received().filter((m) => m.type === "result"), [
      { type: "result", req: "a1", data: {} },
      { type: "result", req: "a2", data: { outcome: "already_placed" } },
    ]);
  });
});

test("runHost sends host_state to attached clients when the state changes", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    assert.deepEqual(hostStates(c), [{ type: "host_state", state: "idle", name: "the lead", clients: 1, held_ui: 0 }]);
    c.send(submit("r1"));
    await r.drain();
    r.finish[0]!("done");
    await r.drain();
    assert.deepEqual(hostStates(c), [
      { type: "host_state", state: "idle", name: "the lead", clients: 1, held_ui: 0 },
      { type: "host_state", state: "working", name: "the lead", clients: 1, held_ui: 0 },
      { type: "host_state", state: "idle", name: "the lead", clients: 1, held_ui: 0 },
    ]);
  });
});

test("runHost sends bye shutdown to clients before it reports stopped and returns 143", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    assert.equal(await r.stop(), 143);
    assert.deepEqual(c.received().at(-1), { type: "bye", reason: "shutdown", reconnect: true });
    assert.deepEqual(r.stateCalls().at(-1), ["session", "state", "stopped", "--session", CONFIG.session]);
    const bye = r.order.indexOf("bye");
    assert.ok(bye >= 0, "no bye");
    assert.ok(bye < r.order.indexOf("state:stopped"), `bye after stopped: ${r.order.join(", ")}`);
  });
});

test("runHost sends bye storage_failed and returns 1 when the watch ends with a failure", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    r.streams[0]!.settle({ kind: "listener_error", error: new Error("boom") });
    await r.drain();
    assert.equal(r.ending.code, 1);
    assert.deepEqual(c.received().at(-1), { type: "bye", reason: "storage_failed", reconnect: true });
    assert.ok(r.order.indexOf("bye") < r.order.indexOf("state:stopped"), `bye after stopped: ${r.order.join(", ")}`);
    assert.equal(r.listener.closes, 1);
  });
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    r.closeConversation({ kind: "failed", error: new Error("disk I/O error") });
    await r.drain();
    assert.equal(r.ending.code, 1);
    assert.deepEqual(c.received().at(-1), { type: "bye", reason: "storage_failed", reconnect: true });
  });
});

test("runHost sends bye storage_failed and returns 1 when the watch delivers a failed call", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    const c = await r.attach();
    await r.streams[0]!.fail(new Error("boom"));
    await r.drain();
    assert.equal(r.ending.code, 1);
    assert.deepEqual(c.received().at(-1), { type: "bye", reason: "storage_failed", reconnect: true });
    assert.ok(r.order.indexOf("bye") < r.order.indexOf("state:stopped"), `bye after stopped: ${r.order.join(", ")}`);
    assert.equal(r.listener.closes, 1);
  });
});

test("runHost closes the listener when it ends", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.ready();
    assert.equal(r.listener.closes, 0);
    assert.equal(await r.stop(), 143);
    assert.equal(r.listener.closes, 1);
    assert.ok(r.order.indexOf("close") < r.order.indexOf("state:stopped"), "closed before stopped is reported");
  });
});
