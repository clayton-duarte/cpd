import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, ID, OTHER, sessionShow, stateCalls, waitFor } from "./fake.ts";

/** commands.md §15.15 (hs-d552, hs-d571): the adapter's loop guard counts self-started turns, trips, gates, reports blocked, resumes and survives a reload. */

const LABEL = "blocked:φ: loop guard";
const WINDOW_MS = 600_000;
const TURN = { ok: true, session: ID, taken: [] };
let nextId = 200;

/** One clock for the adapter, which the test moves. */
const clock = { now: 1_000_000 };

async function start(f: Fixture, over: Record<string, unknown> = {}, entries: unknown[] = []) {
  clock.now = 1_000_000;
  f.pi.entries = entries;
  f.respond("session-show", sessionShow({ loop_guard_turns: 20, ...over }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.respond("board-take", TURN);
  f.load({ now: () => clock.now });
  await f.pi.fire("session_start");
  await waitFor("the first state", () => stateCalls(f).length >= 1);
  await waitFor("the follower", () => f.follows().length > 0);
}

/** A run a cpd message started (`messageType: "custom"`), as pi fires it: `before_agent_start`, then `agent_start`. */
async function selfTurn(f: Fixture, times = 1) {
  for (let i = 0; i < times; i++) {
    await f.pi.fire("before_agent_start", { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } });
    await f.pi.fire("agent_start");
  }
}

/** A run the User typed. */
async function userTurn(f: Fixture) {
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "go" });
  await f.pi.fire("agent_start");
}

/** What a message that would start a turn is sent with: a `job-failed` Event, delivered to a Plan Lead with a turn. */
async function probe(f: Fixture): Promise<{ triggerTurn?: boolean; deliverAs?: string }> {
  const id = nextId++;
  f.emit(ev(id, "job-failed", { data: {} }));
  const sent = await waitFor(`the message of Event ${id}`, () => f.pi.sent.find((s) => (s.message.details as any)?.id === id));
  return sent.options;
}

const TURNS = { triggerTurn: true, deliverAs: "followUp" };
const GATED = { triggerTurn: false, deliverAs: "nextTurn" };
const guardEntries = (f: Fixture) => f.pi.entries.filter((e: any) => e.type === "custom" && e.customType === "cpd-loop-guard").map((e: any) => e.data);
const settled = async (f: Fixture) => {
  await f.quiet();
  return stateCalls(f);
};

test("loop guard: the 20th self-started turn does not trip, the 21st does", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);

  await selfTurn(f, 20);
  assert.deepEqual(await probe(f), TURNS, "20 turns: not tripped");
  assert.deepEqual(guardEntries(f), []);
  assert.ok(!(await settled(f)).includes(LABEL));

  await selfTurn(f);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  assert.deepEqual(await probe(f), GATED, "the 21st turn tripped it");
  const [entry] = guardEntries(f);
  assert.equal(guardEntries(f).length, 1);
  assert.equal(entry.tripped, true);
  assert.equal(entry.turns, 21);
  assert.equal(typeof entry.at, "number");
});

test("loop guard: the limit is session show's loop_guard_turns", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f, { loop_guard_turns: 3 });

  await selfTurn(f, 3);
  assert.deepEqual(await probe(f), TURNS, "3 turns of 3: not tripped");
  await selfTurn(f);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  assert.deepEqual(await probe(f), GATED, "the 4th turn tripped it");
});

test("loop guard: turns older than 10 minutes leave the window", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);

  await selfTurn(f, 20);
  clock.now += WINDOW_MS + 1;
  await selfTurn(f, 20); // the first 20 are out of the window: it holds 20
  assert.deepEqual(await probe(f), TURNS, "not tripped: the old turns left");
  assert.ok(!(await settled(f)).includes(LABEL));

  await selfTurn(f); // 21 inside the window
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  assert.deepEqual(await probe(f), GATED);
});

test("loop guard: a User prompt empties the window", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);

  await selfTurn(f, 20);
  await userTurn(f);
  await selfTurn(f, 20); // 20 since the prompt: the User's turn is not counted either
  assert.deepEqual(await probe(f), TURNS);
  assert.ok(!(await settled(f)).includes(LABEL));

  await selfTurn(f);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  assert.deepEqual(await probe(f), GATED);
});

test("loop guard: a worker is never counted", async (t) => {
  const w = new Fixture();
  try {
    await start(w, { role: "worker", plan_lead: false, attempt: "a1" });
    await selfTurn(w, 30);
    w.emit(ev(nextId++, "attempt-nudge", { session: ID, data: { prompt: "Submit your work." } }));
    const nudge = await waitFor("the nudge", () => w.pi.sent.find((s) => (s.message.details as any)?.kind === "attempt-nudge"));
    assert.deepEqual(nudge.options, TURNS, "a Worker's nudge still starts a turn");
    assert.deepEqual(guardEntries(w), []);
    assert.ok(!(await settled(w)).includes(LABEL));
  } finally {
    await w.close();
  }
  // the control: the same 30 turns trip a Lead, so the Worker's silence is the rule and not a missing clock
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);
  await selfTurn(f, 30);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
});

test("loop guard: a tripped breaker reports blocked with the label φ: loop guard", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);
  await selfTurn(f, 21);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  assert.equal(stateCalls(f).at(-1), LABEL);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "blocked", "--label", "φ: loop guard", "--session", ID]);
});

test("loop guard: a message that would start a turn is sent with triggerTurn false and nextTurn while tripped", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);
  await selfTurn(f, 21);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));

  const first = nextId++;
  const second = nextId++;
  f.emit(ev(first, "job-failed", { data: {} }), ev(second, "session-message", { session: ID, data: { from: null, text: "hi", wake: false } }));
  await waitFor("both delivered", () => f.pi.delivered().length === 2);
  const [a, b] = f.pi.delivered();
  assert.deepEqual([(a!.message.details as any).id, (b!.message.details as any).id], [first, second], "they keep their order");
  assert.deepEqual(a!.options, GATED);
  assert.deepEqual(b!.options, GATED);
});

test("loop guard: the User's prompt runs while tripped, clears the trip and unblocks", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);
  await selfTurn(f, 21);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));

  await userTurn(f);
  await waitFor("working again", () => stateCalls(f).at(-1) === "working");
  assert.deepEqual(guardEntries(f).map((e: any) => e.tripped), [true, false]);
  assert.deepEqual(await probe(f), TURNS, "deliveries start turns again");
});

test("loop guard: session-resumed clears the trip and delivers one message with a turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);
  await selfTurn(f, 21);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  const queued = nextId++;
  f.emit(ev(queued, "job-failed", { data: {} }));
  await waitFor("the queued message", () => f.pi.delivered().length === 1);

  // another Session's resume changes nothing
  f.emit(ev(nextId++, "session-resumed", { session: OTHER, text: `Loop guard of Session ${OTHER} resumed.` }));
  await f.drain();
  assert.equal(stateCalls(f).at(-1), LABEL);
  assert.equal(f.pi.delivered().filter((s) => (s.message.details as any).kind === "session-resumed").length, 0);

  const id = ++nextId;
  f.emit(ev(id, "session-resumed", { session: ID, text: `Loop guard of Session ${ID} resumed.` }));
  const sent = await waitFor("the resume message", () => f.pi.sent.find((s) => (s.message.details as any)?.id === id));
  assert.equal(sent.message.content, `Loop guard of Session ${ID} resumed.`);
  assert.deepEqual(sent.options, TURNS);
  await waitFor("the trip cleared", () => stateCalls(f).at(-1) !== LABEL);
  assert.deepEqual(guardEntries(f).map((e: any) => e.tripped), [true, false]);
  await f.quiet();
  assert.equal(f.pi.delivered().filter((s) => (s.message.details as any).id === queued).length, 1, "what was queued is not sent again");
  assert.equal(f.pi.sent.filter((s) => (s.message.details as any)?.id === id).length, 1, "one message");

  // the window is empty: the turn the message starts is the first of 20
  await selfTurn(f, 20);
  assert.deepEqual(await probe(f), TURNS);
  await selfTurn(f);
  await waitFor("the blocked state again", () => stateCalls(f).at(-1) === LABEL);
});

test("loop guard: session-resumed while not tripped does nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f);
  f.emit(ev(nextId++, "session-resumed", { session: ID, text: `Loop guard of Session ${ID} resumed.` }));
  await f.drain();
  assert.deepEqual(f.pi.sent, [], "no message, no error");
  assert.deepEqual(guardEntries(f), []);
  assert.deepEqual(stateCalls(f), ["idle"]);

  // the control: the same Event delivers once the breaker is tripped
  await selfTurn(f, 21);
  await waitFor("the blocked state", () => stateCalls(f).includes(LABEL));
  const id = ++nextId;
  f.emit(ev(id, "session-resumed", { session: ID, text: `Loop guard of Session ${ID} resumed.` }));
  const sent = await waitFor("the resume message", () => f.pi.sent.find((s) => (s.message.details as any)?.id === id));
  assert.deepEqual(sent.options, TURNS);
});

test("loop guard: a trip survives a reload", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const trip = { type: "custom", customType: "cpd-loop-guard", data: { tripped: true, at: 1, turns: 21 } };
  // an Event waits for the follower before the session starts: the gate is shut before it is delivered
  const waiting = nextId++;
  f.emit(ev(waiting, "job-failed", { data: {} }));
  await start(f, {}, [trip]);
  await waitFor("blocked at once", () => stateCalls(f).includes(LABEL));
  const sent = await waitFor("the waiting message", () => f.pi.sent.find((s) => (s.message.details as any)?.id === waiting));
  assert.deepEqual(sent.options, GATED);
  assert.deepEqual(await probe(f), GATED);

  // the last entry decides: a trip cleared before the reload stays cleared
  const g = new Fixture();
  try {
    await start(g, {}, [trip, { type: "custom", customType: "cpd-loop-guard", data: { tripped: false } }]);
    assert.deepEqual(await probe(g), TURNS);
    assert.ok(!(await settled(g)).includes(LABEL));
  } finally {
    await g.close();
  }
});

test("loop guard: session show without loop_guard_turns is bad_output", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", sessionShow()); // the default answer has no loop_guard_turns
  f.load({ now: () => clock.now });
  await f.pi.fire("session_start");
  const [m] = await waitFor("the error", () => (f.pi.errors().length ? f.pi.errors() : undefined));
  assert.match(m!.message.content, /^φ adapter: session show: bad_output: /);
});
