import assert from "node:assert/strict";
import { test } from "node:test";
import { Ring } from "../../durable-host/ring.ts";

const view = (n: number) => ({ conversation: "c", entries: [], docs: {}, n });
const ev = (n: number) => [["p", ["entries"], n, 0, [{ id: String(n) }]]];
const fill = (r: Ring, n: number) => {
  for (let i = 0; i < n; i++) r.append(ev(i));
};

test("Ring starts with the first view at seq 1", () => {
  const r = new Ring("e1", view(0));
  assert.equal(r.head, 1);
  assert.deepEqual(r.attach(), {
    mode: "view",
    fromSeq: 1,
    headSeq: 1,
    frames: [{ type: "view", epoch: "e1", seq: 1, view: view(0) }],
  });
});

test("append stamps ops frames with the epoch and the next seq", () => {
  const r = new Ring("e1", view(0));
  assert.deepEqual(r.append(ev(1)), { type: "ops", epoch: "e1", seq: 2, ops: ev(1) });
  assert.deepEqual(r.append(ev(2)), { type: "ops", epoch: "e1", seq: 3, ops: ev(2) });
  assert.equal(r.head, 3);
});

test("attach without a cursor gives view mode with the base frame then every ring frame", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  r.append(ev(2));
  const a = r.attach();
  assert.equal(a.mode, "view");
  assert.equal(a.fromSeq, 1);
  assert.equal(a.headSeq, 3);
  assert.deepEqual(a.frames, [
    { type: "view", epoch: "e1", seq: 1, view: view(0) },
    { type: "ops", epoch: "e1", seq: 2, ops: ev(1) },
    { type: "ops", epoch: "e1", seq: 3, ops: ev(2) },
  ]);
  assert.equal(r.head, 3, "a view attach does not mutate the ring");
  assert.deepEqual(r.attach(), a);
});

test("attach with a cursor inside the ring gives resume mode with only the missed frames", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  r.append(ev(2));
  r.append(ev(3));
  assert.deepEqual(r.attach({ epoch: "e1", seq: 2 }), {
    mode: "resume",
    fromSeq: 3,
    headSeq: 4,
    frames: [
      { type: "ops", epoch: "e1", seq: 3, ops: ev(2) },
      { type: "ops", epoch: "e1", seq: 4, ops: ev(3) },
    ],
  });
  assert.equal(r.attach({ epoch: "e1", seq: 1 }).fromSeq, 2);
});

test("attach at the head gives resume mode with no frame", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  assert.deepEqual(r.attach({ epoch: "e1", seq: 2 }), { mode: "resume", fromSeq: 3, headSeq: 2, frames: [] });
});

test("attach with another epoch gives view mode", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  const a = r.attach({ epoch: "e0", seq: 2 });
  assert.equal(a.mode, "view");
  assert.equal(a.fromSeq, 1);
  assert.equal(a.frames.length, 2);
});

test("attach with a seq below the base gives view mode", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  r.rebase(view(1)); // base is now seq 3
  r.append(ev(2));
  const a = r.attach({ epoch: "e1", seq: 2 });
  assert.equal(a.mode, "view");
  assert.equal(a.fromSeq, 3);
  assert.equal(a.headSeq, 4);
  assert.deepEqual(a.frames.map((f) => f.seq), [3, 4]);
  assert.equal(r.attach({ epoch: "e1", seq: 0 }).mode, "view");
});

test("attach with a seq above the head gives view mode", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  const a = r.attach({ epoch: "e1", seq: 3 });
  assert.equal(a.mode, "view");
  assert.equal(a.fromSeq, 1);
  assert.equal(a.headSeq, 2);
});

test("attach reports fromSeq and headSeq", () => {
  const r = new Ring("e1", view(0));
  fill(r, 4);
  assert.deepEqual(
    [r.attach().fromSeq, r.attach().headSeq, r.attach({ epoch: "e1", seq: 3 }).fromSeq, r.attach({ epoch: "e1", seq: 3 }).headSeq],
    [1, 5, 4, 5],
  );
});

test("shouldRebase is true over 256 frames at a run end only", () => {
  const r = new Ring("e1", view(0));
  fill(r, 256);
  assert.equal(r.shouldRebase(true), false);
  assert.equal(r.shouldRebase(false), false);
  r.append(ev(0));
  assert.equal(r.shouldRebase(true), true);
  assert.equal(r.shouldRebase(false), false);
});

test("shouldRebase is true over 4096 frames at any time", () => {
  const r = new Ring("e1", view(0));
  fill(r, 4096);
  assert.equal(r.shouldRebase(false), false);
  r.append(ev(0));
  assert.equal(r.shouldRebase(false), true);
  assert.equal(r.shouldRebase(true), true);
});

test("rebase replaces the base view, empties the ring and continues the seq", () => {
  const r = new Ring("e1", view(0));
  r.append(ev(1));
  r.append(ev(2));
  assert.deepEqual(r.rebase(view(9)), { type: "view", epoch: "e1", seq: 4, view: view(9) });
  assert.equal(r.head, 4);
  assert.equal(r.shouldRebase(true), false);
  assert.deepEqual(r.attach(), {
    mode: "view",
    fromSeq: 4,
    headSeq: 4,
    frames: [{ type: "view", epoch: "e1", seq: 4, view: view(9) }],
  });
  assert.deepEqual(r.append(ev(3)), { type: "ops", epoch: "e1", seq: 5, ops: ev(3) });
  assert.deepEqual(r.attach({ epoch: "e1", seq: 4 }), {
    mode: "resume",
    fromSeq: 5,
    headSeq: 5,
    frames: [{ type: "ops", epoch: "e1", seq: 5, ops: ev(3) }],
  });
});

test("a batch holding a replace op is appended and kept unchanged", () => {
  const r = new Ring("e1", view(0));
  const batch = [["r", { entries: [1] }], ["a", ["x"], "y"]];
  assert.deepEqual(r.append(batch), { type: "ops", epoch: "e1", seq: 2, ops: batch });
  assert.deepEqual(r.attach({ epoch: "e1", seq: 1 }).frames, [{ type: "ops", epoch: "e1", seq: 2, ops: batch }]);
});
