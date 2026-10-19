import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, ID, planShow, settle, stateCalls, waitFor } from "./fake.ts";

/** hs-d278: a run that answered the User ends with `session state idle --reply`; no other run does. */

async function started(f: Fixture) {
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("idle", () => stateCalls(f).length >= 1);
}

/** The `session state` argvs after start-up's `idle`. */
const states = (f: Fixture) => f.argvs().filter((a) => a[1] === "state").slice(1);

const IDLE = ["session", "state", "idle", "--session", ID];
const IDLE_REPLY = ["session", "state", "idle", "--reply", "--session", ID];
const WORKING = ["session", "state", "working", "--session", ID];

/** A run from `before_agent_start` (with `start`) to `agent_settled`. */
async function runOf(f: Fixture, start: Record<string, unknown>, during: () => Promise<void> = async () => {}) {
  await f.pi.fire("before_agent_start", start);
  f.pi.idle = false;
  await f.pi.fire("agent_start");
  await during();
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false });
}

const eventStart = { messageType: "custom", message: { customType: "cpd", details: { ...ev(5, "job-merged"), delivered_to: ID } } };
const sessionMessageStart = (from: string | null) => ({
  messageType: "custom",
  message: { customType: "cpd", details: { ...ev(6, "session-message", { session: ID, data: { from, text: "hi", wake: true } }), delivered_to: ID } },
});

test("a run started by a typed prompt ends with --reply", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await runOf(f, { messageType: "user", prompt: "hello" });
  await waitFor("working and idle", () => states(f).length === 2);
  assert.deepEqual(states(f), [WORKING, IDLE_REPLY]);
});

test("a run started by an Event message ends without --reply", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await runOf(f, eventStart);
  await waitFor("working and idle", () => states(f).length === 2);
  assert.deepEqual(states(f), [WORKING, IDLE]);
});

test("a run started by a User session-message (from null) ends with --reply", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await runOf(f, sessionMessageStart(null));
  await waitFor("working and idle", () => states(f).length === 2);
  assert.deepEqual(states(f), [WORKING, IDLE_REPLY]);
});

test("a run started by a session-message from a Session ends without --reply", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await runOf(f, sessionMessageStart("11111111-2222-4333-8444-555555555555"));
  await waitFor("working and idle", () => states(f).length === 2);
  assert.deepEqual(states(f), [WORKING, IDLE]);
});

test("typing during an Event-started run makes it a reply run; the next run starts clean", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await runOf(f, eventStart, () => f.pi.fire("input", { source: "interactive", text: "wait", streamingBehavior: "steer" }));
  await waitFor("working and idle", () => states(f).length === 2);
  assert.deepEqual(states(f), [WORKING, IDLE_REPLY]);

  await settle(); // the first run's calls are done
  await runOf(f, eventStart);
  await waitFor("the second run", () => states(f).length === 4);
  assert.deepEqual(states(f).slice(2), [WORKING, IDLE], "the mark was cleared at the run start");
});

test("input that is not typed (an extension's), or typed with no run, does not make a reply run", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await f.pi.fire("input", { source: "interactive", text: "at rest" }); // no run: the prompt's own before_agent_start decides
  await runOf(f, eventStart, () => f.pi.fire("input", { source: "extension", text: "/x" }));
  await waitFor("working and idle", () => states(f).length === 2);
  assert.deepEqual(states(f), [WORKING, IDLE]);
});

test("a settled that leaves pi busy reports nothing; the run's final idle carries --reply once", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "hello" });
  await f.pi.fire("agent_start");
  f.pi.idle = false;
  await f.pi.fire("agent_settled", { aborted: false }); // a queued follow-up keeps pi busy
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false });
  await f.pi.fire("agent_settled", { aborted: false }); // again: nothing new
  await waitFor("working and idle", () => states(f).length === 2);
  await settle();
  assert.deepEqual(states(f), [WORKING, IDLE_REPLY]);
});
