import assert from "node:assert/strict";
import { test } from "node:test";
import { line, withHost } from "./rig.ts";

/**
 * Bug b59: the WebUI Esc sends `abort`; the host calls `conversation.abort`, but the in-flight submission's `wait()` is
 * the only thing that returns the host to idle. The mock conversation's abort does not settle that wait (the rig's
 * `finish` is never called), so the host itself must report idle once the abort has been served.
 */
test("an abort from the WebUI stops a running turn and the host returns to idle", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.up();
    const client = r.connect();
    client.send({ type: "hello", protocol: 2, min_protocol: 2, client: "cpd-web" });
    client.send({ type: "attach", req: "attach-1" });
    await r.drain();
    client.send({ type: "submit", req: "s1", text: "run a long turn", when_busy: "followUp" });
    await r.until(() => r.finish.length >= 1 && r.states().includes("working"));
    const before = client.received().length;
    client.send({ type: "abort", req: "a1" });
    await r.until(() => r.aborts.length >= 1);
    await r.until(() => client.received().length > before);
    await r.drain();
    assert.deepEqual(r.aborts, [{ background: false }]);
    assert.equal(r.states().at(-1), "idle", `the host never returned to idle after abort; states: ${r.states().join(",")}`);
  });
});

test("abort cancels a request still preparing and preserves the next request's accounting", { timeout: 20_000 }, async () => {
  let block = false;
  let release!: () => void;
  let started!: () => void;
  const preparing = new Promise<void>((resolve) => { started = resolve; });
  const held = new Promise<void>((resolve) => { release = resolve; });
  await withHost({ beforeEntries: async () => { if (block) { started(); await held; } } }, async (r) => {
    await r.up();
    const client = await r.attach();
    block = true;
    client.send({ type: "submit", req: "old", text: "cancel me", when_busy: "followUp" });
    await preparing;
    client.send({ type: "abort", req: "abort" });
    await r.until(() => r.aborts.length === 1);
    await r.drain();
    assert.equal(r.states().at(-1), "idle");
    release();
    await r.drain();
    assert.equal(r.submitted.length, 0, "the aborted prompt must never reach the conversation");
    block = false;
    client.send({ type: "submit", req: "new", text: "next turn", when_busy: "followUp" });
    await r.until(() => r.finish.length === 1);
    r.finish[0]!("done");
    await r.drain();
    assert.equal(r.states().at(-1), "idle", "the stale catch must not decrement the new pending count");
  });
});


for (const kind of ["pr-feedback", "session-message"]) {
  test(`aborting preparation of a followed ${kind} keeps the host available`, { timeout: 20_000 }, async () => {
    let block = false;
    let release!: () => void;
    let started!: () => void;
    const preparing = new Promise<void>((r) => { started = r; });
    const held = new Promise<void>((r) => { release = r; });
    await withHost({ beforeEntries: async () => { if (block) { started(); await held; } } }, async (r) => {
      const hooks = await r.up();
      const client = await r.attach();
      block = true;
      hooks.onLine(line(901, kind, kind === "session-message" ? { data: { text: "cancel", from: null } } : {}));
      await preparing;
      client.send({ type: "abort", req: "abort-event" });
      await r.until(() => r.aborts.length === 1);
      await r.drain();
      release();
      await r.drain();
      assert.equal(r.ending.code, undefined, "expected cancellation does not end the host");
      assert.equal(r.submitted.length, 0);
      block = false;
      hooks.onLine(line(902, "pr-feedback"));
      await r.until(() => r.finish.length === 1);
      r.finish[0]!("done");
      await r.drain();
      assert.equal(r.states().at(-1), "idle");
    });
  });
}
