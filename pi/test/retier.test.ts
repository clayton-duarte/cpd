import assert from "node:assert/strict";
import { test } from "node:test";
import { LEAD_MESSAGES } from "../lib/delivery.ts";
import { KINDS, PROTOCOL } from "../lib/event.ts";
import { ev, Fixture, ID, OTHER, planShow, shown, waitFor } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

// hs-d506, commands.md §15.3 `session-retiered`: a Session whose Tier `cpd session retier` changed switches pi's model and thinking level at its next
// idle, once, with no message and no restart. The model and level are the ones `cpd session show` reports (`model`, `thinking`, commands.md §10.2); the adapter
// reads no config. Each test here fails now because the adapter does not know the kind: it ignores the Event and calls neither `setModel` nor `setThinkingLevel`.

/** Long enough for the adapter's one `cpd session show` and the calls after it. */
const WAIT = 5000;
const retiered = (id: number, session: string = ID) => ev(id, "session-retiered", { session, data: { from: "O3", to: "O2" } });
/** The row after the retier: `O2` is `tiers.O` at `medium`. */
const shownO2 = (over: Record<string, unknown> = {}) => shown({ tier: "O2", model: "p/opus", thinking: "medium", ...over });

async function started(t: { after(fn: () => Promise<void>): void }, show: Record<string, unknown> = {}): Promise<Fixture> {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown(show));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.respond("session-show", shownO2(show)); // the row `session retier` wrote
  return f;
}

test("at idle after a retier the adapter sets the tier's model and thinking level, once, with no message", async (t) => {
  const f = await started(t);
  f.emit(retiered(200));
  await waitFor("setModel", () => f.pi.modelsSet.length > 0, WAIT);
  await f.drain();
  assert.deepEqual(f.pi.modelsSet, [{ provider: "p", id: "opus" }], "provider/model-id resolved through ctx.modelRegistry.find");
  assert.deepEqual(f.pi.thinkingSet, ["medium"]);
  assert.deepEqual(f.pi.sent, [], "no message and no turn: the switch is silent");
  await f.pi.fire("agent_settled");
  await f.quiet();
  assert.equal(f.pi.modelsSet.length, 1, "once per change: a later idle sets nothing");
  assert.equal(f.pi.thinkingSet.length, 1);
});

test("not while working: the switch waits for the idle and then happens once", async (t) => {
  const f = await started(t);
  await f.pi.busy();
  f.emit(retiered(200));
  await f.drain();
  await f.quiet();
  assert.deepEqual([f.pi.modelsSet, f.pi.thinkingSet], [[], []], "a working Session is never switched");

  f.pi.idle = true;
  await f.pi.fire("agent_settled");
  await waitFor("setModel at idle", () => f.pi.modelsSet.length > 0, WAIT);
  await f.quiet();
  assert.deepEqual(f.pi.modelsSet, [{ provider: "p", id: "opus" }]);
  assert.deepEqual(f.pi.thinkingSet, ["medium"]);
  await f.pi.fire("agent_settled");
  await f.quiet();
  assert.equal(f.pi.modelsSet.length, 1);
  assert.equal(f.pi.thinkingSet.length, 1);
});

test("two retiers before one idle make one switch, to what the row says at that idle", async (t) => {
  const f = await started(t);
  await f.pi.busy();
  f.emit(retiered(200), { ...retiered(201), data: { from: "O2", to: "O1" } });
  await f.drain();
  f.respond("session-show", shown({ tier: "O1", model: "p/opus", thinking: "low" }));
  f.pi.idle = true;
  await f.pi.fire("agent_settled");
  await waitFor("setModel at idle", () => f.pi.modelsSet.length > 0, WAIT);
  await f.quiet();
  assert.deepEqual([f.pi.modelsSet.length, f.pi.thinkingSet], [1, ["low"]]);
});

test("an Event for another Session changes nothing, and the Session's own then does", async (t) => {
  const f = await started(t);
  f.emit(retiered(200, OTHER));
  await f.drain();
  await f.pi.fire("agent_settled");
  await f.quiet();
  assert.deepEqual([f.pi.modelsSet, f.pi.thinkingSet], [[], []]);
  f.emit(retiered(201));
  await waitFor("setModel", () => f.pi.modelsSet.length > 0, WAIT);
  await f.quiet();
  assert.deepEqual([f.pi.modelsSet.length, f.pi.thinkingSet], [1, ["medium"]]);
});

test("a Manager switches too", async (t) => {
  const f = await started(t, { role: "manager", plan: null, plan_lead: false, name: "ctx-manager" });
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false, tier: "O4", model: "p/opus", thinking: "xhigh" }));
  f.emit({ ...retiered(200), plan: null, data: { from: "O5", to: "O4" } });
  await waitFor("setModel", () => f.pi.modelsSet.length > 0, WAIT);
  // a Manager gets only what is addressed to it: a message to it is the marker that the Event before it was handled
  await f.drain((id) => ev(id, "session-message", { session: ID, plan: null, data: { from: null, text: "x", wake: false } }));
  assert.deepEqual([f.pi.modelsSet, f.pi.thinkingSet], [[{ provider: "p", id: "opus" }], ["xhigh"]]);
});

/** The `cpd session switch-failed` and `cpd session switched` calls that started (commands.md §10.16), in order. */
const switchCalls = (f: Fixture, sub: "switch-failed" | "switched") => f.argvs().filter((a) => a[0] === "session" && a[1] === sub);
const flag = (argv: string[], name: string) => argv[argv.indexOf(name) + 1];

/** The failed switch was recorded once, with the text the adapter reports, and `switched` never ran. */
function assertRecorded(f: Fixture, why: RegExp) {
  const calls = switchCalls(f, "switch-failed");
  assert.equal(calls.length, 1, "switch-failed runs exactly once");
  const argv = calls[0]!;
  assert.equal(flag(argv, "--code"), "model_unavailable");
  assert.equal(flag(argv, "--session"), ID);
  const message = flag(argv, "--message");
  assert.match(message, /p\/opus/, "the message names the model");
  assert.match(message, why);
  assert.equal(message, adapterErrors(f)[0]!.message.content.replace(/^φ adapter: model switch: model_unavailable: /, ""), "the same text the adapter reports");
  assert.deepEqual(switchCalls(f, "switched"), [], "switched never runs after a failure");
}

test("a model pi cannot find is one error message and sets nothing, not even the level", async (t) => {
  const f = await started(t);
  f.pi.knownModels.clear();
  f.emit(retiered(200));
  const e = await waitFor("an error", () => adapterErrors(f)[0], WAIT);
  assert.equal(e.code, "model_unavailable");
  await f.drain();
  assert.deepEqual([f.pi.modelsSet, f.pi.thinkingSet, adapterErrors(f).length], [[], [], 1]);
  assertRecorded(f, /./);
});

test("a setModel that answers false (no authentication) is one error message and sets no level", async (t) => {
  const f = await started(t);
  f.pi.modelAccepted = false;
  f.emit(retiered(200));
  const e = await waitFor("an error", () => adapterErrors(f)[0], WAIT);
  assert.equal(e.code, "model_unavailable");
  await f.drain();
  assert.deepEqual([f.pi.thinkingSet, adapterErrors(f).length], [[], 1]);
  assertRecorded(f, /no authentication/);
});

test("a setModel that throws records the failed switch once", async (t) => {
  const f = await started(t);
  f.pi.setModel = async () => {
    throw new Error("provider exploded");
  };
  f.emit(retiered(200));
  const e = await waitFor("an error", () => adapterErrors(f)[0], WAIT);
  assert.equal(e.code, "model_unavailable");
  await f.drain();
  assert.deepEqual([f.pi.thinkingSet, adapterErrors(f).length], [[], 1]);
  assertRecorded(f, /provider exploded/);
});

test("a successful switch runs switched after the model and level are set", async (t) => {
  const f = await started(t);
  f.emit(retiered(200));
  await waitFor("switched", () => switchCalls(f, "switched").length > 0, WAIT);
  await f.drain();
  assert.deepEqual([f.pi.modelsSet, f.pi.thinkingSet], [[{ provider: "p", id: "opus" }], ["medium"]]);
  assert.deepEqual(switchCalls(f, "switched"), [["session", "switched", "--session", ID]]);
  assert.deepEqual(switchCalls(f, "switch-failed"), []);
  assert.deepEqual(adapterErrors(f), []);
});

test("a failed session show records nothing", async (t) => {
  const f = await started(t);
  f.respond("session-show", { ok: false, error: { code: "session_not_found", message: "no" } }, { exit: 4 });
  f.emit(retiered(200));
  await f.drain();
  await f.quiet();
  assert.deepEqual([f.pi.modelsSet, f.pi.thinkingSet], [[], []]);
  assert.deepEqual([switchCalls(f, "switch-failed"), switchCalls(f, "switched")], [[], []], "the process state is unknown: nothing is recorded");

  // A later switch that works is the only one recorded.
  f.respond("session-show", shownO2());
  f.emit(retiered(201));
  await waitFor("switched", () => switchCalls(f, "switched").length > 0, WAIT);
  await f.quiet();
  assert.deepEqual(switchCalls(f, "switched"), [["session", "switched", "--session", ID]]);
  assert.deepEqual(switchCalls(f, "switch-failed"), []);
});

test("the protocol is exactly one past the base", () => {
  assert.equal(PROTOCOL, 20, "attempt-aborted raised it to 20; context pending's plan-level jobs raised it to 19; session-model-changed raised it to 18; pr-described raised it to 17; the commands the adapter runs grew it earlier (contract §5.2)");
});

test("the protocol was raised past 8 and session-retiered is a known kind delivered as no message", () => {
  assert.ok(PROTOCOL > 8, "adding a kind the adapter acts on raises protocol (contract §5.2): the next free number on the PR's base");
  assert.ok((KINDS as readonly string[]).includes("session-retiered"), "session-retiered is not in KINDS");
  assert.equal((LEAD_MESSAGES as Record<string, unknown>)["session-retiered"], undefined, "no message, no turn");
});
