import type { ChildProcess, spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture, ID, planShow, ev, waitFor } from "./fake.ts";

/**
 * The follower's restart scheduler: it records each delay asked for and runs a restart only when the TEST says so, so no restart waits for real time.
 * (`Options.schedule`; the handle it returns is never a real timer.)
 */
function manual() {
  const delays: number[] = [];
  const queue: (() => void)[] = [];
  return {
    /** Every delay asked for, in order. */
    delays,
    /** The restarts scheduled and not yet run. */
    pending: () => queue.length,
    schedule: ((run: () => void, ms: number) => (delays.push(ms), queue.push(run), {} as NodeJS.Timeout)) as (run: () => void, ms: number) => NodeJS.Timeout,
    /** Waits for the follower to ask for a restart (its end was handled), then runs the oldest one. */
    async restart(): Promise<void> {
      await waitFor("a restart to be scheduled", () => queue.length > 0);
      queue.shift()!();
    },
  };
}

async function started(f: Fixture, s: ReturnType<typeof manual>, options = {}) {
  f.respond("plan-show", planShow());
  f.load({ schedule: s.schedule, ...options });
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

const failure = (code: string) => JSON.stringify({ ok: false, error: { code, message: `${code} happened.`, details: { x: 1 } } });
/** A Lead's adapter errors are reported to the Manager, never delivered to its chat (commands.md §15.1, hs-d573). */
const errorsOf = (f: Fixture, code: string) => f.reports().filter((r) => r.code === code);

test("EOF restarts the follower with the in-memory cursor, also past Events handled without a message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const s = manual();
  await started(f, s);
  f.emit(ev(101, "job-added"), ev(102, "session-state"));
  f.exitNext(0); // the fake prints every line of the file before it exits, so both are read first
  await s.restart();
  await waitFor("a restart", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 102]);
  assert.deepEqual(f.pi.sent, [], "a clean EOF is no error, and the ignored kinds are no message");
});

test("restarts back off from restartInitialMs up to restartMaxMs", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  // The delays the follower asks for, not the intervals measured (those include the start-up of a process under load).
  const s = manual();
  await started(f, s);
  for (let n = 2; n <= 5; n++) {
    f.exitNext(0);
    await s.restart();
    await waitFor(`start ${n}`, () => f.follows().length === n);
  }
  assert.deepEqual(s.delays.slice(0, 4), [30, 60, 120, 120], "doubling from restartInitialMs, capped at restartMaxMs");
});

test("a failure line is delivered once per code until a line is read again; the restart goes on", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const s = manual();
  await started(f, s);
  f.failFollower(failure("store_too_new"), 3);
  await s.restart();
  await s.restart();
  await waitFor("three starts", () => f.follows().length >= 3);
  await waitFor("the error", () => errorsOf(f, "store_too_new").length === 1);
  const e = errorsOf(f, "store_too_new")[0]!;
  assert.deepEqual([e.what, e.message], ["events follow", "store_too_new happened."]);
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
  assert.ok(f.adapterLog().some((l) => / error: events follow: store_too_new: store_too_new happened\.$/.test(l)), "logged");
  assert.equal(errorsOf(f, "follower_exit").length, 0, "the exit after a failure line adds nothing");
  assert.equal(errorsOf(f, "store_too_new").length, 1, "once, however often it restarts");

  f.clearFollowerFailure();
  await s.restart(); // the third follower failed too: the next one starts with no failure and reads the Event
  f.emit(ev(101, "job-merged", { data: { turn: true } }));
  await waitFor("a line is read again", () => f.pi.delivered().length === 1);
  f.failFollower(failure("store_too_new"), 3);
  await s.restart();
  await waitFor("restarted again", () => f.follows().length >= 5);
  assert.equal(errorsOf(f, "store_too_new").length, 1, "the same what and code is not shown again (§15.1)");
});

test("a permanent failure line is reported once and the follower is not restarted (pi-ci-W4)", async (t) => {
  for (const code of ["session_not_found", "usage"]) {
    const f = new Fixture();
    try {
      // The restarts the follower asks for: a restart is scheduled when the follower's end is handled, so none once it has exited is none at all.
      const s = manual();
      await started(f, s);
      f.failFollower(failure(code), 4);
      await waitFor("the error", () => errorsOf(f, code).length === 1);
      await f.exited();
      await f.quiet();
      assert.deepEqual(s.delays, [], `${code}: no restart scheduled`);
      assert.equal(f.follows().length, 1, `${code}: no restart`);
      assert.equal(errorsOf(f, code).length, 1, `${code}: reported once`);
    } finally {
      await f.close();
    }
  }
});

test("a non-zero exit without a failure line is delivered as an error, once per code", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const s = manual();
  await started(f, s);
  f.exitNext(2);
  await waitFor("the error", () => errorsOf(f, "follower_exit").length === 1);
  assert.deepEqual([errorsOf(f, "follower_exit")[0]!.what, errorsOf(f, "follower_exit")[0]!.session], ["events follow", ID]);
  assert.match(errorsOf(f, "follower_exit")[0]!.message!, /exit 2/);
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
  await s.restart();
  f.exitNext(2);
  await s.restart();
  await waitFor("a third start", () => f.follows().length >= 3); // the second exit was reported (or not) before this start
  assert.equal(errorsOf(f, "follower_exit").length, 1);
});

test("a malformed line is an error and is skipped; the cursor moves past it", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const s = manual();
  await started(f, s);
  f.emit(
    "this is not json",
    { ...ev(101, "job-merged", { data: { turn: true } }), at: 5 },
    { ...ev(102, "job-merged"), data: "not an object" },
    { ...ev(103, "escalation"), data: {} },
    ev(104, "job-merged", { data: { turn: true } }),
  );
  await waitFor("the good event after four bad lines", () => f.pi.delivered().length === 1);
  await f.quiet();
  assert.equal(f.reports().length, 1, "bad lines with the same what and code are reported once (§15.1)");
  for (const r of f.reports()) assert.deepEqual([r.what, r.code], ["events follow", "invalid_event"]);
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
  assert.equal((f.pi.delivered()[0]!.message.details as any).id, 104);

  f.exitNext(0);
  await s.restart();
  await waitFor("restart", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 104]);

  // a bad line with an id alone: the restart skips past it too
  f.emit({ ...ev(110, "job-merged", { data: { turn: true } }), at: 5 });
  f.exitNext(0);
  await s.restart();
  await waitFor("restart", () => f.follows().length === 3);
  assert.equal(f.sinces()[2], 110);
});

test("an Event of an unknown kind is skipped silently; the cursor moves past it (hs-d230)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const s = manual();
  await started(f, s);
  f.emit(ev(101, "job-merged", { data: { turn: true } }), { ...ev(102, "job-merged", { data: { turn: true } }), kind: "from-the-future" }, ev(103, "job-merged", { data: { turn: true } }));
  // Events are handled in order: 103 delivered means 102 was handled before it
  await waitFor("both known Events", () => f.pi.delivered().length === 2);
  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [101, 103]);
  assert.equal(f.pi.errors().length, 0, "no message for the unknown kind");

  // the cursor is past the unknown Event: an unknown kind last still moves it
  f.emit({ ...ev(104, "job-merged", { data: { turn: true } }), kind: "from-the-future" });
  f.exitNext(0); // the fake prints every line emitted before it, then exits: 104 is read before the restart
  await s.restart();
  await waitFor("restart", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 104]);
  assert.equal(f.pi.errors().length, 0);
});

/** `spawn` of a program that does not exist, in process: a child that emits `error` with ENOENT and then `close`, as node's does. */
function missing(): ChildProcess {
  const child = Object.assign(new EventEmitter(), { stdout: null, stderr: null, pid: undefined, kill: () => false });
  queueMicrotask(() => {
    child.emit("error", Object.assign(new Error("spawn cpd ENOENT"), { code: "ENOENT", errno: -2, syscall: "spawn cpd" }));
    child.emit("close", -2, null);
  });
  return child as unknown as ChildProcess;
}

test("a follower that cannot be started is an error, once, and is retried", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  const s = manual();
  let starts = 0;
  f.load({
    schedule: s.schedule,
    spawn: (() => (starts++, missing())) as unknown as typeof spawn,
  });
  await f.pi.fire("session_start");
  await s.restart();
  await s.restart();
  await waitFor("retries", () => starts >= 3);
  await waitFor("the report", () => errorsOf(f, "spawn_failed").length > 0);
  await s.restart(); // a fourth start: still one report
  await waitFor("the fourth start", () => starts >= 4);
  await f.quiet();
  assert.equal(errorsOf(f, "spawn_failed").length, 1);
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
  assert.deepEqual(s.delays.slice(0, 3), [30, 60, 120], "each retry backs off");
});

test("session_shutdown kills the follower and it is not restarted", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const s = manual();
  await started(f, s);
  await waitFor("pid", () => f.pids().length === 1);
  await f.pi.fire("session_shutdown");
  await waitFor("killed", () => f.killed(f.pids()[0]!));
  await f.exited();
  assert.equal(f.follows().length, 1);
  assert.deepEqual(s.delays, [], "no restart asked for");
  assert.equal(f.pi.errors().length, 0, "a killed follower at shutdown is no error");
});
