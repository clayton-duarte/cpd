import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture, planShow, waitFor } from "./fake.ts";

// commands.md §15.2: a `store_too_new` failure line of a follower started before an install is not delivered (the restart runs the new cpd).

const failure = JSON.stringify({ ok: false, error: { code: "store_too_new", message: "store_too_new happened.", details: { x: 1 } } });
const errorsOf = (f: Fixture) => f.pi.errors().filter((e) => (e.message.details as any).code === "store_too_new");

async function started(f: Fixture, delays: number[]) {
  f.respond("plan-show", planShow());
  f.load({ schedule: (run, ms) => (delays.push(ms), setTimeout(run, 0)) });
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

test("store_too_new from a follower started before an install is not delivered and the follower restarts", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const delays: number[] = [];
  await started(f, delays);
  f.file("id-less-once", "1"); // the new cpd no longer fails: the restarted follower does not print the line again
  f.reinstall(); // the run started with identity A; the installed cpd is now B
  f.emit(failure);
  f.file("exit-next", "3"); // the old cpd prints the line and exits
  await waitFor("the restart", () => f.follows().length === 2);
  await f.quiet();
  assert.deepEqual(errorsOf(f), [], "nothing is delivered");
  assert.equal(f.pi.errors().length, 0, "the exit that follows adds nothing");
  assert.deepEqual(delays, [30], "restarted after the initial delay");
});

test("store_too_new with the same installed cpd is still delivered once", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const delays: number[] = [];
  await started(f, delays);
  f.emit(failure);
  f.file("exit-next", "3");
  await waitFor("the restart", () => f.follows().length === 2);
  await waitFor("the error", () => errorsOf(f).length === 1);
  f.emit(failure);
  f.file("exit-next", "3");
  await waitFor("a third start", () => f.follows().length === 3);
  await f.quiet();
  assert.equal(errorsOf(f).length, 1, "once per code");
});
