import assert from "node:assert/strict";
import { test } from "node:test";
import { withHost } from "./rig.ts";

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
