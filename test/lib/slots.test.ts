import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { acquireSlot, releaseSlot, setSlotPid } from "../../src/lib/slots.ts";
import { slotPath } from "../../src/lib/paths.ts";
import { sandboxEnv } from "../helpers/index.ts";

const fresh = () => {
  process.env.CPD_HOME = sandboxEnv().CPD_HOME!;
};
const who = (attempt: string) => ({ wf: "pay-4521", attempt });

function deadPid(): Promise<number> {
  return new Promise((resolve) => {
    const c = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
    c.on("close", () => resolve(c.pid!));
  });
}

test("with concurrency 2 a third acquire returns null; releasing frees a slot", async () => {
  fresh();
  assert.equal(await acquireSlot(2, who("a-a1")), 0);
  assert.equal(await acquireSlot(2, who("b-a1")), 1);
  assert.equal(await acquireSlot(2, who("c-a1")), null);
  assert.equal(await releaseSlot(0, "a-a1"), true);
  assert.equal(existsSync(slotPath(0)), false);
  assert.equal(await acquireSlot(2, who("c-a1")), 0);
});

test("a slot file holds {wf, attempt, pid, at}", async () => {
  fresh();
  await acquireSlot(1, who("a-a1"));
  const s = JSON.parse(readFileSync(slotPath(0), "utf8"));
  assert.deepEqual(Object.keys(s).sort(), ["at", "attempt", "pid", "wf"]);
  assert.equal(s.wf, "pay-4521");
  assert.equal(s.attempt, "a-a1");
  assert.equal(s.pid, process.pid);
});

test("a slot held by a dead pid is reclaimed", async () => {
  fresh();
  assert.equal(await acquireSlot(1, who("old-a1")), 0);
  writeFileSync(slotPath(0), JSON.stringify({ wf: "pay-4521", attempt: "old-a1", pid: await deadPid(), at: new Date().toISOString() }));
  assert.equal(await acquireSlot(1, who("new-a1")), 0);
  assert.equal(JSON.parse(readFileSync(slotPath(0), "utf8")).attempt, "new-a1");
});

test("release never frees a slot held by another attempt", async () => {
  fresh();
  await acquireSlot(1, who("a-a1"));
  await assert.rejects(releaseSlot(0, "b-a1"), (e: any) => e.code === "slot_not_held" && e.exitCode === 3);
  assert.equal(JSON.parse(readFileSync(slotPath(0), "utf8")).attempt, "a-a1");
});

test("releasing a free slot is a no-op returning false", async () => {
  fresh();
  assert.equal(await releaseSlot(0, "a-a1"), false);
});

test("setSlotPid hands the slot to another process; a dead one makes it reclaimable", async () => {
  fresh();
  await acquireSlot(1, who("a-a1"));
  await setSlotPid(0, "a-a1", await deadPid());
  assert.equal(await acquireSlot(1, who("b-a1")), 0);
  await assert.rejects(setSlotPid(0, "a-a1", process.pid), (e: any) => e.code === "slot_not_held");
});

test("a corrupt slot file fails loud", async () => {
  fresh();
  await acquireSlot(1, who("a-a1"));
  writeFileSync(slotPath(0), "garbage");
  await assert.rejects(acquireSlot(1, who("b-a1")), (e: any) => e.code === "slot_corrupt");
});

test("concurrent acquires never hand out the same slot", async () => {
  fresh();
  const got = await Promise.all([0, 1, 2, 3, 4, 5].map((i) => acquireSlot(3, who(`j${i}-a1`))));
  const taken = got.filter((x) => x !== null).sort();
  assert.deepEqual(taken, [0, 1, 2]);
  assert.equal(got.filter((x) => x === null).length, 3);
});
