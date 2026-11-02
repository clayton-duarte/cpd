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

// ---- commands.md §15.15 L8: the intra-turn tool-call guard ----

type Verdict = { block?: boolean; reason?: string } | undefined;
let callId = 0;
const call = async (f: Fixture, toolName: string, input: unknown, nested = false): Promise<Verdict> => {
  const toolCallId = nested ? `p/${callId++}` : `c${callId++}`;
  return (await f.pi.fireWith("tool_call", nested ? { toolName, toolCallId, parentToolCallId: "p", input } : { toolName, toolCallId, input }))[0] as Verdict;
};
const refused = (v: Verdict) => v?.block === true && typeof v.reason === "string" && v.reason.startsWith("φ: loop guard:");
const READ_REASON =
  "φ: loop guard: more than 12 consecutive read-only tool calls in one turn. Stop your turn now. Delegate codebase exploration to a scout Job (cpd job add <k> --kind scout), or run the cpd state command that records your decision.";
const TOTAL_REASON =
  "φ: loop guard: more than 30 tool calls in one turn. Stop your turn now: end it with what you have, and delegate any further codebase exploration to a scout Job.";
const bash = (f: Fixture, command: string, nested = false) => call(f, "bash", { command }, nested);
const reads = async (f: Fixture, n: number, command = "rg foo") => {
  const out: Verdict[] = [];
  for (let i = 0; i < n; i++) out.push(await bash(f, command));
  return out;
};
const reset = (f: Fixture) => f.pi.fire("before_agent_start", { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } });
const passes = (vs: Verdict[], what: string) => vs.forEach((v, i) => assert.ok(!refused(v), `${what}: call ${i + 1} passes`));
/** 12 reads pass, the 13th is refused: the streak is at zero before. */
async function twelveThenRefused(f: Fixture, what: string, command = "rg foo") {
  passes(await reads(f, 12, command), what);
  assert.ok(refused(await bash(f, command)), `${what}: the 13th is refused`);
}
/** A fresh counter set: reset, then 12 reads pass. */
async function freshStart(t: { after(fn: () => unknown): void }, over: Record<string, unknown> = {}) {
  const f = new Fixture();
  t.after(() => f.close());
  await start(f, over);
  return f;
}

test("loop guard intra-turn: the 12th consecutive read-only call passes and the 13th is refused", async (t) => {
  const f = await freshStart(t);
  for (let i = 1; i <= 12; i++) {
    const v = i % 2 ? await call(f, "read", { path: "a.md" }) : await bash(f, "rg foo");
    assert.ok(!refused(v), `call ${i} passes`);
  }
  const v = await call(f, "read", { path: "a.md" });
  assert.ok(refused(v), "call 13 is refused");
  assert.match(v!.reason!, /^φ: loop guard:/);
});

test("loop guard intra-turn: the refusal reason names the read streak bound and the way out", async (t) => {
  const f = await freshStart(t);
  passes(await reads(f, 12), "reads");
  const v = await bash(f, "rg foo");
  assert.deepEqual(v, { block: true, reason: READ_REASON });
});

test("loop guard intra-turn: a state-mutating cpd command in bash or codemode resets the read streak", async (t) => {
  const f = await freshStart(t);
  for (const run of [
    () => bash(f, "cpd job add x --title t"),
    () => call(f, "codemode", { code: 'await tools.bash({ command: "cpd plan lock" })' }),
    () => bash(f, "cpd job show a && cpd job cancel a --reason r"),
  ]) {
    passes(await reads(f, 12), "reads");
    assert.ok(refused(await bash(f, "rg foo")), "the 13th read is refused");
    await reset(f);
    passes(await reads(f, 12), "reads");
    const w = await run();
    assert.ok(!refused(w), "the state write passes");
    passes(await reads(f, 12), "reads after the write");
    assert.ok(refused(await bash(f, "rg foo")), "the 13th read after the write is refused");
    await reset(f);
  }
});

test("loop guard intra-turn: write, edit, cpd_plan_review and ask_user_question reset the read streak", async (t) => {
  const f = await freshStart(t);
  const tools: [string, unknown][] = [
    ["write", { path: "/tmp/cpd-loop-guard-x.md", content: "x" }],
    ["edit", { path: "/tmp/cpd-loop-guard-x.md", edits: [] }],
    ["cpd_plan_review", { title: "t" }],
    ["ask_user_question", { questions: [{ question: "q", options: ["a", "b"] }] }],
  ];
  for (const [tool, input] of tools) {
    passes(await reads(f, 12), tool);
    assert.ok(refused(await bash(f, "rg foo")), `${tool}: the 13th read is refused`);
    await reset(f);
    passes(await reads(f, 12), tool);
    assert.ok(!refused(await call(f, tool, input)), `${tool} is not refused`);
    passes(await reads(f, 12), `${tool}: after`);
    assert.ok(refused(await bash(f, "rg foo")), `${tool}: the streak restarted at the write`);
    await reset(f);
  }
});

test("loop guard intra-turn: read-only cpd commands and other tools count as reads", async (t) => {
  const f = await freshStart(t);
  const cmds = ["cpd plan show", "cpd job show j", "cpd context show --since 3", "cpd context search q", "git status"];
  for (const c of cmds) {
    await twelveThenRefused(f, c, c);
    await reset(f);
  }
  for (let i = 0; i < 12; i++) assert.ok(!refused(await call(f, "read", { path: "a" })));
  assert.ok(refused(await call(f, "read", { path: "a" })), "read tool");
  await reset(f);
  for (let i = 0; i < 12; i++) assert.ok(!refused(await call(f, "codemode", { code: 'await tools.bash({ command: "cpd plan show" })' })));
  assert.ok(refused(await call(f, "codemode", { code: 'await tools.read({ path: "a" })' })), "codemode that only reads");
  await reset(f);
  for (const c of ["cpd job showcase", "cpd jobs add", "echo cpd-job add"]) {
    await twelveThenRefused(f, c, c);
    await reset(f);
  }
});

const STATE_WRITES = [
  "job add|edit|cancel|retry|steer|addendum|urgent|approve-tests|approve-touches",
  "workflow add|edit|describe|cancel|start",
  "plan new|phase|outline|lock|review|rename|move|done|cancel|archive",
  "pr open|merge|post|rebase|push|describe|reply|rerun",
  "context new|decide|note|ask|answer|brief|plan|rename|archive|delete",
  "session send|close|resume|rename|fork",
  "escalation answer",
  "review submit",
  "inbox claim",
  "preset apply",
  "board post",
].flatMap((g) => {
  const [group, subs] = [g.split(" ")[0]!, g.split(" ")[1]!];
  return subs.split("|").map((s) => `cpd ${group} ${s} x`);
});

test("loop guard intra-turn: every listed state-mutating cpd subcommand resets the read streak", async (t) => {
  const f = await freshStart(t);
  const all = [...STATE_WRITES, "cpd answer j-a1-q1 ok"];
  assert.equal(all.length, 9 + 5 + 10 + 8 + 10 + 5 + 1 + 1 + 1 + 1 + 1 + 1);
  // a control: 12 reads and a read-only command leave the 13th refused
  await twelveThenRefused(f, "control");
  await reset(f);
  for (const c of all) {
    passes(await reads(f, 12), c);
    assert.ok(!refused(await bash(f, c)), `${c} passes`);
    assert.ok(!refused(await bash(f, "rg foo")), `${c} reset the streak: read 1 passes`);
    await reset(f);
  }
});

test("loop guard intra-turn: the 31st tool call of a turn is refused with the total calls reason", async (t) => {
  const f = await freshStart(t);
  for (let i = 1; i <= 30; i++) assert.ok(!refused(await bash(f, "cpd job add x --title t")), `call ${i} passes`);
  assert.deepEqual(await bash(f, "cpd job add x --title t"), { block: true, reason: TOTAL_REASON });
  await reset(f);

  // both bounds exceeded: the total-calls reason wins
  for (let i = 0; i < 18; i++) await bash(f, "cpd job add x --title t");
  passes(await reads(f, 12), "reads up to 30");
  assert.deepEqual(await bash(f, "rg foo"), { block: true, reason: TOTAL_REASON });
});

test("loop guard intra-turn: after the 30th call even a state-mutating call is refused", async (t) => {
  const f = await freshStart(t);
  for (let i = 0; i < 12; i++) await bash(f, "rg foo");
  for (let i = 0; i < 18; i++) await bash(f, i % 6 === 5 ? "cpd job add x --title t" : "rg foo");
  assert.deepEqual(await bash(f, "cpd job add x --title t"), { block: true, reason: TOTAL_REASON });
});

test("loop guard intra-turn: a call made inside a codemode script is neither counted nor refused", async (t) => {
  const f = await freshStart(t);
  for (let i = 0; i < 100; i++) assert.ok(!refused(await bash(f, "rg foo", true)), "nested read");
  passes(await reads(f, 12), "direct reads after 100 nested");
  assert.ok(refused(await bash(f, "rg foo")), "the 13th direct read is refused");
  await reset(f);
  // the parent is classified by its own code
  passes(await reads(f, 12), "reads");
  assert.ok(!refused(await call(f, "codemode", { code: 'await tools.bash({ command: "cpd plan lock" })' })), "parent state write passes");
  passes(await reads(f, 12), "reads after the parent");
  assert.ok(refused(await bash(f, "rg foo")));
  // a nested call is not refused even when the direct ones are
  assert.ok(!refused(await bash(f, "rg foo", true)), "nested call while refusing");
});

/** 12 reads then `n` refused reads. */
async function refuse(f: Fixture, n: number) {
  passes(await reads(f, 12), "reads");
  const vs = await reads(f, n);
  vs.forEach((v, i) => assert.ok(refused(v), `refusal ${i + 1}`));
}

test("loop guard intra-turn: the third refusal in one turn trips the breaker", async (t) => {
  const f = await freshStart(t);
  await refuse(f, 3);
  assert.ok((await settled(f)).includes(LABEL), "blocked with the label");
  const entries = guardEntries(f);
  assert.equal(entries.length, 1);
  assert.equal(entries[0].tripped, true);
  assert.equal(entries[0].at, clock.now);
  assert.deepEqual(await probe(f), GATED);
});

test("loop guard intra-turn: two refusals do not trip the breaker", async (t) => {
  const f = await freshStart(t);
  await refuse(f, 2);
  assert.ok(!(await settled(f)).includes(LABEL));
  assert.deepEqual(guardEntries(f), []);
});

test("loop guard intra-turn: a refusal after the trip appends no second entry", async (t) => {
  const f = await freshStart(t);
  await refuse(f, 5);
  assert.ok((await settled(f)).includes(LABEL));
  assert.equal(guardEntries(f).length, 1);
  assert.equal(stateCalls(f).filter((s) => s === LABEL).length, 1);
});

async function resetCounters(f: Fixture, fireReset: (f: Fixture) => Promise<unknown>) {
  await refuse(f, 2);
  await fireReset(f);
  await twelveThenRefused(f, "second turn");
  await fireReset(f);
  // two refusals, a reset, two more refusals: no trip
  await refuse(f, 2);
  await fireReset(f);
  await refuse(f, 2);
  assert.ok(!(await settled(f)).includes(LABEL), "no trip");
  assert.deepEqual(guardEntries(f), []);
}

test("loop guard intra-turn: before_agent_start resets the counters", async (t) => {
  await resetCounters(await freshStart(t), (f) => f.pi.fire("before_agent_start", { messageType: "user", prompt: "go" }));
});

test("loop guard intra-turn: agent_end resets the counters", async (t) => {
  await resetCounters(await freshStart(t), (f) => f.pi.fire("agent_end", { messages: [] }));
});

test("loop guard intra-turn: a Manager is guarded", async (t) => {
  const f = await freshStart(t, { role: "manager", plan_lead: false, plan: null });
  await refuse(f, 3);
  assert.ok((await settled(f)).includes(LABEL));
  assert.equal(guardEntries(f).length, 1);
});

test("loop guard intra-turn: a Worker is never guarded", async (t) => {
  const w = new Fixture();
  try {
    await start(w, { role: "worker", plan_lead: false, attempt: "a1" });
    passes(await reads(w, 60), "worker reads");
    assert.deepEqual(guardEntries(w), []);
    assert.ok(!(await settled(w)).includes(LABEL));
  } finally {
    await w.close();
  }
  // the control: a Lead is refused at the 13th read
  const f = await freshStart(t);
  await twelveThenRefused(f, "lead");
});

test("loop guard intra-turn: the User prompt clears a trip caused by refusals", async (t) => {
  const f = await freshStart(t);
  await refuse(f, 3);
  assert.ok((await settled(f)).includes(LABEL));
  await userTurn(f);
  await waitFor("working again", () => stateCalls(f).at(-1) === "working");
  assert.deepEqual(guardEntries(f).map((e: any) => e.tripped), [true, false]);
  await twelveThenRefused(f, "after the prompt");
});
