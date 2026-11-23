import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import type { Write } from "../../durable-host/turn.ts";
import { CONFIG, entry, line, rig, withHost } from "./rig.ts";

const reviewRecord = (status: string) => ({
  kind: "cpd-plan-review",
  data: { plan: "p1", review: "r1", surface: "chat", status },
});
const reviewEvent = (id: number, status: string, plan = "p1") =>
  line(id, "plan-review", { plan, session: null, data: { review: "r1", status } });

test("restart of a non-current rejected review records history without admitting a Lead turn", { timeout: 20_000 }, async () => {
  await withHost({
    stored: [reviewRecord("pending"), entry(200, "session-message")],
    answers: { "plan-show": { review: { id: "r1", status: "rejected", current: false } } },
  }, async (r) => {
    const hooks = await r.up();
    assert.equal(hooks.cursor(), 200, "the verdict can be behind the persisted follower cursor");
    assert.deepEqual(r.submitted, []);
    assert.ok((r.written as Write[]).some((w) => w.entry.kind === "cpd-plan-review" && w.entry.data.status === "rejected"));
    assert.equal((await r.attach()).received().some((m) => m.type === "ui_request"), false);
  });
});

test("restart admits one current verdict and ignores replay once the review is settled", { timeout: 20_000 }, async () => {
  const answers = { "plan-show": { review: { id: "r1", status: "approved", current: true } } };
  let settled: unknown;
  await withHost({ stored: [reviewRecord("pending"), entry(200, "session-message")], answers }, async (r) => {
    await r.up();
    assert.deepEqual(r.submitted.map((i) => i.requestId), ["plan-decision:p1:r1"]);
    assert.ok(r.order.indexOf("submit:plan-decision:p1:r1") < r.order.indexOf("write:cpd-plan-review"));
    settled = (r.written as Write[]).find((w) => w.entry.kind === "cpd-plan-review" && w.entry.data.status === "approved")?.entry;
    assert.deepEqual(settled, { ...reviewRecord("approved"), model: [] });
  });
  await withHost({ stored: [reviewRecord("pending"), settled], answers }, async (r) => {
    const hooks = await r.up();
    hooks.onLine(reviewEvent(201, "pending"));
    hooks.onLine(reviewEvent(202, "approved"));
    await r.drain();
    assert.deepEqual(r.submitted, []);
    assert.ok((r.written as Write[]).every((w) => !w.entry.kind.startsWith("cpd-plan-")));
  });
});

test("a Plan completion with no session starts its durable Lead exactly once", { timeout: 20_000 }, async () => {
  const r = rig();
  try {
    const hooks = await r.up();
    const completion = line(48357, "job-merged", { session: null, data: { turn: true } });
    hooks.onLine(completion);
    hooks.onLine(completion);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(r.submitted.length, 1, "a filtered Plan event must not be dropped because its subject session is null");
    assert.equal(r.submitted[0]!.requestId, "event:48357");
    assert.equal(r.submitted[0]!.entry.data.delivered_to, CONFIG.session);
    r.finish[0]!("done");
    await r.until(() => r.states().includes("idle"));
    assert.equal(hooks.cursor(), 48357);
  } finally {
    await r.stop();
  }
});

test("runHost starts the follower at the cursor read from the stored entries", { timeout: 20_000 }, async () => {
  const r = rig({ stored: [entry(130, "job-failed"), entry(120, "pr-feedback"), { kind: "cpd-event", data: { id: 400, kind: "job-failed", delivered_to: "the-source" } }] });
  const hooks = await r.up();
  assert.equal(hooks.cursor(), 130);
  await r.stop();
  assert.equal(r.follower.stopped, 1);

  const none = rig({ stored: [entry(30, "job-failed")] });
  assert.equal((await none.up()).cursor(), 100, "registered_event is greater");
  await none.stop();
});

test("runHost submits the input of a followed Event and reports working then idle", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  assert.equal(hooks.onLine(line(150, "pr-feedback")), true);
  await r.until(() => r.submitted.length >= 1);
  assert.deepEqual(r.submitted, [{ content: "text of pr-feedback 150", requestId: "event:150", whenBusy: "steer", entry: entry(150, "pr-feedback") }]);
  assert.equal(hooks.cursor(), 150);
  await r.until(() => r.states().includes("working"));
  r.finish[0]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.states(), ["working", "idle"]);
  await r.stop();
});

test("runHost submits an Event once when the follower delivers it twice", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(151, "job-failed"));
  hooks.onLine(line(151, "job-failed"));
  await r.until(() => r.submitted.length >= 1);
  hooks.onLine(line(152, "job-failed"));
  await r.until(() => r.submitted.length >= 2);
  assert.deepEqual(
    r.submitted.map((i) => i.requestId),
    ["event:151", "event:152"],
  );
  await r.stop();
});

test("runHost reports idle only when no submission is pending", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(160, "job-failed"));
  hooks.onLine(line(161, "job-failed"));
  await r.until(() => r.finish.length >= 2);
  r.finish[0]!("done");
  r.finish[1]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.states(), ["working", "idle"]);
  await r.stop();
});

test("runHost logs a follower failure and keeps running", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onFailure(new CpdError("tool_failed", "events follow exited 1"));
  await r.until(() => r.logs.some((l) => l.includes("events follow exited 1")));
  assert.equal(r.ending.code, undefined);
  hooks.onLine(line(170, "job-failed"));
  await r.until(() => r.submitted.length >= 1);
  assert.equal(await r.stop(), 143);
});

const IDLE = ["session", "state", "idle", "--session", CONFIG.session];
const IDLE_REPLY = ["session", "state", "idle", "--reply", "--session", CONFIG.session];

test("runHost reports idle with reply after a run started by a message from the User", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(200, "session-message", { data: { text: "hello", from: null } }));
  await r.until(() => r.finish.length >= 1);
  r.finish[0]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.stateCalls().at(-1), IDLE_REPLY);
  await r.stop();
});

test("runHost reports idle without reply after a run of Events and messages from Sessions", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(210, "pr-feedback"));
  hooks.onLine(line(211, "job-failed"));
  await r.until(() => r.finish.length >= 2);
  r.finish[0]!("done");
  r.finish[1]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.stateCalls().at(-1), IDLE);
  await r.stop();

  const peer = rig();
  const peerHooks = await peer.up();
  peerHooks.onLine(line(212, "session-message", { data: { text: "hi", from: "s9", wake: false } }));
  await peer.until(() => peer.finish.length >= 1);
  peer.finish[0]!("done");
  await peer.until(() => peer.states().includes("idle"));
  assert.deepEqual(peer.stateCalls().at(-1), IDLE);
  await peer.stop();

  const user = rig();
  const userHooks = await user.up();
  userHooks.onLine(line(213, "session-message", { data: { text: "hello", from: null } }));
  await user.until(() => user.finish.length >= 1);
  user.finish[0]!("done");
  await user.until(() => user.states().includes("idle"));
  assert.deepEqual(user.stateCalls().at(-1), IDLE_REPLY, "contrast: the User's message makes a reply run");
  await user.stop();
});

test("runHost makes a run a reply run when a message from the User joins it", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(220, "pr-feedback"));
  await r.until(() => r.finish.length >= 1);
  hooks.onLine(line(221, "session-message", { data: { text: "typed during the run", from: null } }));
  await r.until(() => r.finish.length >= 2);
  r.finish[0]!("done");
  r.finish[1]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.stateCalls().at(-1), IDLE_REPLY);
  await r.stop();
});

test("runHost reports reply once and the next run without it", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(230, "session-message", { data: { text: "hello", from: null } }));
  await r.until(() => r.finish.length >= 1);
  r.finish[0]!("done");
  await r.until(() => r.states().filter((s) => s === "idle").length >= 1);
  hooks.onLine(line(231, "pr-feedback"));
  await r.until(() => r.finish.length >= 2);
  r.finish[1]!("done");
  await r.until(() => r.states().filter((s) => s === "idle").length >= 2);
  assert.deepEqual(
    r.stateCalls().filter((a) => a[2] === "idle"),
    [IDLE_REPLY, IDLE],
  );
  await r.stop();
});
