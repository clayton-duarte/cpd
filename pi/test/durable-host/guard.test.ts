import assert from "node:assert/strict";
import { test } from "node:test";
import { socketPath } from "../../durable-host/config.ts";
import { HostRunning } from "../../durable-host/sockets.ts";
import { CONFIG, withHost } from "./rig.ts";

const SOCK = socketPath(CONFIG.home, CONFIG.session);
const running = () => new HostRunning(SOCK);

test("runHost calls the guard with the socket path before any other call", async () => {
  await withHost({}, async (r) => {
    await r.ready();
    assert.equal(r.guards.length, 1, "the host never called the guard");
    assert.equal(r.guards[0]!.path, SOCK);
    assert.equal(r.guards[0]!.calls, 0, "a cpd call came before the guard");
    assert.equal(r.guards[0]!.facts, 0, "an effect came before the guard");
  });
});

test("runHost returns 1 and logs host_running when the guard rejects with HostRunning", async () => {
  await withHost({ guard: running() }, async (r) => {
    await r.ready();
    assert.equal(r.ending.code, 1);
    assert.ok(r.logs.some((l) => l.includes("host_running")), `no host_running in ${JSON.stringify(r.logs)}`);
  });
});

test("runHost makes no cpd call and opens no store when the guard rejects with HostRunning", async () => {
  await withHost({ guard: running() }, async (r) => {
    await r.ready();
    assert.equal(r.ending.code, 1, "the host did not end with 1");
    assert.deepEqual(r.calls, []);
    assert.equal(r.opened.n, 0);
  });
});

test("runHost reports no state when the guard rejects with HostRunning, even when the Session shows stopped", async () => {
  await withHost({ guard: running(), sessionOver: { state: "stopped" } }, async (r) => {
    await r.ready();
    assert.equal(r.ending.code, 1, "the host did not end with 1");
    assert.deepEqual(r.stateCalls(), []);
    assert.deepEqual(r.order.filter((f) => f.startsWith("state:")), []);
  });
});

test("runHost throws a guard failure that is not HostRunning before any effect", async () => {
  const failure = new Error("EACCES: run/host");
  await withHost({ guard: failure }, async (r) => {
    await r.ready();
    assert.equal(r.ending.error, failure);
    assert.deepEqual(r.calls, []);
    assert.equal(r.opened.n, 0);
  });
});
