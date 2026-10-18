import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, planShow, settle, waitFor } from "./fake.ts";

async function started(f: Fixture, options = {}) {
  f.respond("plan-show", planShow());
  f.load(options);
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

const failure = (code: string) => JSON.stringify({ ok: false, error: { code, message: `${code} happened.`, details: { x: 1 } } });
const errorsOf = (f: Fixture, code: string) => f.pi.errors().filter((e) => (e.message.details as any).code === code);

test("EOF restarts the follower with the in-memory cursor, also past Events handled without a message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.emit(ev(101, "job-added"), ev(102, "session-state"));
  await settle(80);
  f.file("exit-next", "0");
  await waitFor("a restart", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 102]);
  assert.deepEqual(f.pi.sent, [], "a clean EOF is no error, and the ignored kinds are no message");
});

test("restarts back off from restartInitialMs up to restartMaxMs", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  // The delays the follower asks for, not the intervals measured (those include the start-up of a process under load).
  const delays: number[] = [];
  f.respond("plan-show", planShow());
  f.load({ schedule: (run, ms) => (delays.push(ms), setTimeout(run, 0)) });
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  for (let n = 2; n <= 5; n++) {
    f.file("exit-next", "0");
    await waitFor(`start ${n}`, () => f.follows().length === n);
  }
  assert.deepEqual(delays.slice(0, 4), [30, 60, 120, 120], "doubling from restartInitialMs, capped at restartMaxMs");
});

test("a failure line is delivered once per code until a line is read again; the restart goes on", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.file("follow-failure.json", JSON.stringify({ line: failure("store_too_new"), exit: 3 }));
  await waitFor("three starts", () => f.follows().length >= 3);
  await waitFor("the error", () => errorsOf(f, "store_too_new").length === 1);
  const e = errorsOf(f, "store_too_new")[0]!;
  assert.equal(e.message.content, "cpd adapter: events follow: store_too_new: store_too_new happened.");
  assert.deepEqual(e.options, { triggerTurn: true, deliverAs: "followUp" });
  assert.equal(errorsOf(f, "follower_exit").length, 0, "the exit after a failure line adds nothing");
  assert.equal(errorsOf(f, "store_too_new").length, 1, "once, however often it restarts");

  f.remove("follow-failure.json");
  f.emit(ev(101, "job-merged"));
  await waitFor("a line is read again", () => f.pi.delivered().length === 1);
  f.file("follow-failure.json", JSON.stringify({ line: failure("store_too_new"), exit: 3 }));
  await waitFor("delivered again", () => errorsOf(f, "store_too_new").length === 2);
});

test("a permanent failure line is reported once and the follower is not restarted (pi-ci-W4)", async (t) => {
  for (const code of ["session_not_found", "usage"]) {
    const f = new Fixture();
    try {
      await started(f);
      f.file("follow-failure.json", JSON.stringify({ line: failure(code), exit: 4 }));
      await waitFor("the error", () => errorsOf(f, code).length === 1);
      await settle(500); // restarts would come after 30, 60 and 120 ms
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
  await started(f);
  f.file("exit-next", "2");
  await waitFor("the error", () => errorsOf(f, "follower_exit").length === 1);
  assert.match(errorsOf(f, "follower_exit")[0]!.message.content, /^cpd adapter: events follow: follower_exit: .*exit 2/);
  f.file("exit-next", "2");
  await waitFor("a third start", () => f.follows().length >= 3);
  await settle(100);
  assert.equal(errorsOf(f, "follower_exit").length, 1);
});

test("a line that is not a valid Event is an error and is skipped; the cursor moves past it", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.emit(
    "this is not json",
    { ...ev(101, "job-merged"), kind: "no-such-kind" },
    { ...ev(102, "job-merged"), data: "not an object" },
    { ...ev(103, "escalation"), data: {} },
    ev(104, "job-merged"),
  );
  await waitFor("the good event after four bad lines", () => f.pi.delivered().length === 1);
  assert.equal(f.pi.errors().length, 4, "one error per bad line");
  for (const e of f.pi.errors()) assert.match(e.message.content, /^cpd adapter: events follow: invalid_event: /);
  assert.equal((f.pi.delivered()[0]!.message.details as any).id, 104);

  f.file("exit-next", "0");
  await waitFor("restart", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 104]);

  // a bad line with an id alone: the restart skips past it too
  f.emit({ ...ev(110, "job-merged"), kind: "nope" });
  await waitFor("its error", () => f.pi.errors().some((e) => /unknown kind "nope"/.test(e.message.content)));
  f.file("exit-next", "0");
  await waitFor("restart", () => f.follows().length === 3);
  assert.equal(f.sinces()[2], 110);
});

test("a follower that cannot be started is an error, once, and is retried", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  let starts = 0;
  f.load({
    spawn: ((_cmd: string, _args: string[], o: object) => {
      starts++;
      return spawn("cpd-does-not-exist", [], o);
    }) as typeof spawn,
  });
  await f.pi.fire("session_start");
  await waitFor("retries", () => starts >= 3);
  assert.equal(errorsOf(f, "spawn_failed").length, 1);
});

test("session_shutdown kills the follower and it is not restarted", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await waitFor("pid", () => f.pids().length === 1);
  await f.pi.fire("session_shutdown");
  await waitFor("killed", () => f.killed(f.pids()[0]!));
  await settle(200);
  assert.equal(f.follows().length, 1);
  assert.equal(f.pi.errors().length, 0, "a killed follower at shutdown is no error");
});
