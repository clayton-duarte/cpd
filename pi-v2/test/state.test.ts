import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, ID, planShow, settle, stateCalls, waitFor } from "./fake.ts";

async function started(f: Fixture) {
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("idle", () => stateCalls(f).length >= 1);
}

test("agent_start reports working; agent_settled reports idle only when pi is idle", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);

  await f.pi.fire("agent_start");
  await waitFor("working", () => stateCalls(f).length === 2);
  f.pi.idle = false; // settled, but a queued follow-up keeps pi busy
  await f.pi.fire("agent_settled", { aborted: false });
  await settle();
  assert.deepEqual(stateCalls(f), ["idle", "working"]);

  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false });
  await waitFor("idle", () => stateCalls(f).length === 3);
  assert.deepEqual(stateCalls(f), ["idle", "working", "idle"]);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "idle", "--session", ID], "idle takes no --label");
});

test("blocking signals of other extensions are counted; the label is the latest opened; a stray close changes nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);

  const signal = (active: boolean, label?: string) => f.pi.events.emit("herdr:blocked", active ? { active, label } : { active });
  signal(false); // nothing open: nothing closes
  await settle();
  assert.deepEqual(stateCalls(f), ["idle"]);

  signal(true, "dialog one");
  await waitFor("blocked", () => stateCalls(f).length === 2);
  signal(true, "dialog two");
  await waitFor("relabelled", () => stateCalls(f).length === 3);
  signal(false);
  await waitFor("one closed", () => stateCalls(f).length === 4);
  signal(false);
  await waitFor("all closed", () => stateCalls(f).length === 5);
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog one", "blocked:dialog two", "blocked:dialog one", "idle"]);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state")[1], ["session", "state", "blocked", "--label", "dialog one", "--session", ID]);
});

test("blocked beats working; when the last reason closes during a turn the state is working", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await f.pi.fire("agent_start");
  await waitFor("working", () => stateCalls(f).length === 2);
  f.pi.events.emit("herdr:blocked", { active: true, label: "ask" });
  await waitFor("blocked", () => stateCalls(f).length === 3);
  f.pi.events.emit("herdr:blocked", { active: false });
  await waitFor("working again", () => stateCalls(f).length === 4);
  assert.deepEqual(stateCalls(f), ["idle", "working", "blocked:ask", "working"]);
});

test("calls are serialised: one in flight, only the latest wanted state is sent next", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: true }, { delay: 120 });

  await f.pi.fire("agent_start"); // working: in flight
  f.pi.events.emit("herdr:blocked", { active: true, label: "x" }); // superseded
  f.pi.events.emit("herdr:blocked", { active: false });
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false }); // idle: the latest
  await waitFor("two more calls", () => f.argvs().filter((a) => a[1] === "state").length === 3, 3000);
  await settle(300);

  assert.deepEqual(stateCalls(f), ["idle", "working", "idle"], "blocked never went out");
  const calls = f.calls().filter((c) => c.argv[1] === "state");
  const ends = calls.filter((c) => c.phase === "end");
  const starts = calls.filter((c) => c.phase === "start");
  for (let i = 1; i < starts.length; i++) assert.ok(starts[i]!.t >= ends[i - 1]!.t, `call ${i} started before call ${i - 1} ended`);
});

test("a refused call is reported once and the next change is tried normally", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: false, error: { code: "illegal_transition", message: "stopped to working.", details: {} } }, { exit: 3 });

  await f.pi.fire("agent_start");
  await waitFor("the error", () => f.pi.errors().length === 1);
  assert.match(f.pi.errors()[0]!.message.content, /^cpd adapter: session state: illegal_transition: stopped to working\.$/);
  await settle();
  assert.equal(f.pi.errors().length, 1, "once");

  f.respond("session-state", { ok: true });
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false });
  await waitFor("the next change", () => stateCalls(f).length === 3);
  assert.deepEqual(stateCalls(f), ["idle", "working", "idle"]);
  assert.equal(f.pi.errors().length, 1);
});

test("session_shutdown reports stopped and stops the follower", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await waitFor("the follower", () => f.pids().length === 1);
  await f.pi.fire("session_shutdown");
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "stopped", "--session", ID]);
  await waitFor("the follower killed", () => f.killed(f.pids()[0]!));
  await f.pi.fire("agent_start"); // after shutdown nothing is reported
  await settle();
  assert.equal(stateCalls(f).at(-1), "stopped");
});

test("session_shutdown waits at most shutdownWaitMs for `stopped`", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: true }, { delay: 3000 });
  const began = Date.now();
  await f.pi.fire("session_shutdown");
  assert.ok(Date.now() - began < 1500, `took ${Date.now() - began} ms`);
  assert.equal(stateCalls(f).at(-1), "stopped");
});

test("a /name typed in pi reaches cpd2; a name cpd2 set does not echo back", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  assert.equal(f.pi.sessionName, "the lead");

  await f.pi.fire("session_info_changed", { name: "the lead" });
  await settle();
  assert.equal(f.argvs().some((a) => a[1] === "rename"), false);

  f.pi.sessionName = "typed by the User";
  await f.pi.fire("session_info_changed", { name: "typed by the User" });
  await waitFor("the rename", () => f.argvs().some((a) => a[1] === "rename"));
  assert.deepEqual(f.argvs().find((a) => a[1] === "rename"), ["session", "rename", "typed by the User", "--session", ID]);

  await f.pi.fire("session_info_changed", { name: "typed by the User" });
  await settle();
  assert.equal(f.argvs().filter((a) => a[1] === "rename").length, 1);

  // the Event it causes comes back and changes nothing
  f.emit(ev(101, "session-renamed", { session: ID, data: { name: "typed by the User" } }));
  await waitFor("applied", () => f.pi.notices.length === 1);
  assert.equal(f.argvs().filter((a) => a[1] === "rename").length, 1);
});
