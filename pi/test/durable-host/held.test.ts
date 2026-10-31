import assert from "node:assert/strict";
import { test } from "node:test";
import { ProtocolError } from "../../durable-host/protocol.ts";
import { answer, create, heldCount, heldList, resolveAll, resolveByHost } from "../../durable-host/held.ts";

// Contract §6.15 `held.ts`: pure tests over the map, no socket, no timer. A fake "clock": `create` takes the `ms` and
// the test fires the expiry itself by calling `resolveByHost(id, "timeout")`, exactly as the host resolves an expired
// request. Every test drives the stub through a call that must be `create`/`answer`/`resolveByHost`/`resolveAll` (or a
// map read), so each fails on 'not implemented' while the types pin the contract.

const record = () => {
  const outcomes: { outcome: string; by: string }[] = [];
  return {
    outcomes,
    onResolve: (outcome: "answered" | "cancelled" | "timeout" | "superseded" | "aborted", by: string) => {
      outcomes.push({ outcome, by });
    },
  };
};

const settle = async (p: Promise<object | undefined>, value?: object) => {
  let resolved: object | undefined;
  let result: "pending" | "resolved" | "rejected" = "pending";
  p.then(
    (v) => {
      resolved = v;
      result = "resolved";
    },
    () => {
      result = "rejected";
    },
  );
  await new Promise((r) => setImmediate(r));
  anchor(value);
  return { resolved, result };
};
// `anchor` names a value so the closure above is not flagged dead; it runs nothing.
const anchor = (v: object | undefined) => v;

test("create holds an entry with a fresh id, the expiry timestamp and the purpose", () => {
  const rec = record();
  const before = heldCount();
  const { id, promise } = create("select", { title: "Pick", options: ["Allow once", "Deny"] }, "secret_approve", { ms: 5000, onResolve: rec.onResolve });
  assert.ok(id.length > 0);
  assert.match(id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
  assert.equal(heldCount(), before + 1);
  const h = heldList().find((e) => e.id === id);
  assert.ok(h);
  assert.equal(h.method, "select");
  assert.equal(h.purpose, "secret_approve");
  assert.equal(h.expires_at, h.created_at + 5000);
  assert.deepEqual(h.fields, { title: "Pick", options: ["Allow once", "Deny"] });
  assert.deepEqual(rec.outcomes, []);
  assert.ok(promise instanceof Promise);
});

test("answer returns answered with the value and removes the entry", async () => {
  const rec = record();
  const { id, promise } = create("select", { title: "Pick", options: ["Allow once", "Deny"] }, "secret_approve", { onResolve: rec.onResolve });
  assert.deepEqual(answer(id, { value: "Allow once" }), { outcome: "answered" });
  const s = await settle(promise);
  assert.equal(s.result, "resolved");
  assert.equal(s.resolved, "Allow once" as unknown as object);
  assert.equal(heldCount(), 0);
  assert.equal(rec.outcomes.length, 1);
  assert.equal(rec.outcomes[0].outcome, "answered");
});

test("answer with cancelled keeps the request held and the promise pending", async () => {
  const rec = record();
  const { id, promise } = create("select", { title: "Pick", options: ["Allow once", "Deny"] }, "secret_approve", { onResolve: rec.onResolve });
  assert.deepEqual(answer(id, { cancelled: true }), { outcome: "cancelled" });
  assert.equal(heldCount(), 1);
  const s = await settle(promise);
  assert.equal(s.result, "pending");
});

test("resolveByHost settles the promise with undefined and reports the outcome", async () => {
  const rec = record();
  const { id, promise } = create("select", { title: "Pick", options: ["Allow once", "Deny"] }, "secret_approve", { onResolve: rec.onResolve });
  resolveByHost(id, "superseded");
  const s = await settle(promise);
  assert.equal(s.result, "resolved");
  assert.equal(s.resolved, undefined);
  assert.equal(heldCount(), 0);
  assert.deepEqual(rec.outcomes, [{ outcome: "superseded", by: "host" }]);
});

test("resolveAll settles every held request for the abort path", async () => {
  const rec = record();
  const a = create("select", { title: "a" }, "other", { onResolve: rec.onResolve });
  const b = create("input", { title: "b", placeholder: "x" }, "other", { onResolve: rec.onResolve });
  resolveAll("aborted");
  const sa = await settle(a.promise);
  const sb = await settle(b.promise);
  assert.equal(sa.result, "resolved");
  assert.equal(sa.resolved, undefined);
  assert.equal(sb.result, "resolved");
  assert.equal(sb.resolved, undefined);
  assert.equal(heldCount(), 0);
});

test("an expired request is resolved by the host as timeout and removed", async () => {
  const rec = record();
  const { id, promise } = create("select", { title: "Pick", options: ["a"] }, "other", { ms: 1000, onResolve: rec.onResolve });
  resolveByHost(id, "timeout");
  const s = await settle(promise);
  assert.equal(s.result, "resolved");
  assert.equal(s.resolved, undefined);
  assert.equal(heldCount(), 0);
  assert.deepEqual(rec.outcomes, [{ outcome: "timeout", by: "host" }]);
});

test("a signal fires superseded and removes the request", async () => {
  const rec = record();
  const controller = new AbortController();
  const { id, promise } = create("select", { title: "Pick", options: ["a"] }, "other", { signal: controller.signal, onResolve: rec.onResolve });
  controller.abort();
  const s = await settle(promise);
  assert.equal(s.result, "resolved");
  assert.equal(s.resolved, undefined);
  assert.equal(heldCount(), 0);
  assert.deepEqual(rec.outcomes, [{ outcome: "superseded", by: "host" }]);
});

test("answer for an unknown or already-removed id throws ui_unknown", () => {
  const rec = record();
  const { id } = create("select", { title: "Pick" }, "other", { onResolve: rec.onResolve });
  resolveByHost(id, "aborted");
  const isUnknown = (e: unknown) => e instanceof ProtocolError && e.code === "ui_unknown";
  assert.throws(() => answer(id, { value: "late" }), isUnknown);
  assert.throws(() => answer("never-seen", { value: "y" }), isUnknown);
});

test("heldList is oldest first and editor requests carry no expiry", () => {
  const rec = record();
  const a = create("select", { title: "a", options: [] }, "other", { ms: 100, onResolve: rec.onResolve });
  const b = create("editor", { title: "b" }, "other", { onResolve: rec.onResolve });
  const c = create("input", { title: "c" }, "other", { ms: 100, onResolve: rec.onResolve });
  const list = heldList();
  assert.deepEqual(
    list.map((h) => h.id),
    [a.id, b.id, c.id],
  );
  assert.equal(list[0].expires_at !== undefined, true);
  assert.equal(list[1].expires_at, undefined);
  assert.equal(list[2].expires_at !== undefined, true);
});