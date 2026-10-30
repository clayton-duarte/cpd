import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, FakePi, ID, OTHER, planShow, shown, waitFor } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

// commands.md §15.2 step 5a (MCP replay), contract §5.3 (pro-d226): at every adapter start the adapter reads its own Session's `mcp-attached` Events with one
// `cpd events list` and registers the latest of each name in id order. Each test is red now because the adapter does not replay.

const cfg = (tag: string) => ({ url: `https://mcp.example.com/${tag}`, exposure: "hidden" });
const A = cfg("a");
const B = cfg("b");
const C = cfg("c");
const attached = (id: number, name: string, server: unknown, session: string = ID) => ev(id, "mcp-attached", { session, data: { name, server } });
const LIST_ARGV = ["events", "list", "--session", ID, "--kind", "mcp-attached", "--limit", "5000"];

type Role = "lead" | "manager" | "worker";
const SHOWN: Record<Role, Record<string, unknown>> = {
  lead: {},
  manager: { role: "manager", plan: null, plan_lead: false },
  worker: { role: "worker", plan_lead: false },
};

/** A Fixture whose ledger answers `events: ...` to the replay read; not yet started. */
function ledger(t: { after(fn: () => Promise<void>): void }, events: unknown[], role: Role = "lead", over: Record<string, unknown> = {}, extra: { exit?: number } = {}): Fixture {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown({ ...SHOWN[role], ...over }));
  f.respond("events-list@mcp-attached", { ok: true, events, next: events.length }, extra);
  return f;
}
async function start(f: Fixture): Promise<void> {
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
}
const listCalls = (f: Fixture) => f.calls().filter((c) => c.phase === "start" && c.argv[0] === "events" && c.argv[1] === "list");

test("with no mcp-attached Event the start registers no server", async (t) => {
  const f = ledger(t, []);
  await start(f);
  assert.deepEqual(f.pi.registeredMcp, []);
});

test("start replays the own mcp-attached Events with one events list call", async (t) => {
  const f = ledger(t, [attached(10, "slack", A), attached(11, "atlassian", B)]);
  await start(f);
  const lists = listCalls(f).filter((c) => c.argv.includes("mcp-attached"));
  assert.equal(lists.length, 1);
  assert.deepEqual(lists[0]!.argv, LIST_ARGV);
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: A }, { name: "atlassian", config: B }], "registered when the follower started");
});

test("a reload registers the attached servers again", async (t) => {
  const f = ledger(t, [attached(10, "slack", A), attached(11, "atlassian", B)]);
  await start(f);
  const expected = [{ name: "slack", config: A }, { name: "atlassian", config: B }];
  assert.deepEqual(f.pi.registeredMcp, expected, "first activation");
  await f.pi.fire("session_shutdown", { reason: "reload" });
  const second = new FakePi(f.cpd);
  f.load({}, second);
  await second.fire("session_start");
  await waitFor("the second follower", () => f.follows().length > 1);
  assert.deepEqual(second.registeredMcp, expected, "second activation");
});

test("start registers the latest Event of each name in id order", async (t) => {
  const f = ledger(t, [attached(10, "slack", A), attached(11, "atlassian", B), attached(12, "slack", C)]);
  await start(f);
  assert.deepEqual(f.pi.registeredMcp, [{ name: "atlassian", config: B }, { name: "slack", config: C }]);
});

test("start registers the own Session Event and not another Session Event", async (t) => {
  const f = ledger(t, [attached(10, "slack", A), attached(11, "atlassian", B, OTHER)]);
  await start(f);
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: A }]);
});

test("the follower redelivery of a replayed Event registers nothing", async (t) => {
  const f = ledger(t, [attached(59, "slack", A), attached(60, "slack", B)], "lead", { registered_event: 50 });
  f.emit(attached(59, "slack", A), attached(60, "slack", B)); // the follower prints them again: the cursor is below both
  await start(f);
  f.emit(ev(101, "job-added"));
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: B }], "the replay registered 60 only, the redelivery nothing");
  f.emit(attached(200, "slack", C)); // the control: a new attach registers
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: B }, { name: "slack", config: C }]);
});

test("a failed events list at start is one adapter error and the start goes on", async (t) => {
  const f = ledger(t, [], "lead", {}, { exit: 1 });
  await start(f);
  f.emit(ev(101, "job-added"));
  const errors = adapterErrors(f);
  assert.equal(errors.length, 1, "one adapter error");
  assert.equal(errors[0]!.what, "events list (mcp-attached)");
  assert.deepEqual(f.pi.registeredMcp, []);
  f.emit(attached(200, "slack", A));
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: A }]);
});

test("a refused registration at start is reported and the other servers still register", async (t) => {
  const f = ledger(t, [attached(10, "slack", A), attached(11, "atlassian", B)]);
  f.pi.mcpThrowsFor.add("slack");
  await start(f);
  await f.quiet();
  const errors = adapterErrors(f);
  assert.equal(errors.length, 1);
  assert.equal(errors[0]!.code, "mcp_register_failed");
  assert.equal(f.reports()[0]!.details?.name, "slack");
  assert.ok(f.pi.registeredMcp.some((r) => r.name === "atlassian"), "atlassian registered");
});

test("a full page of mcp-attached Events at start is reported as mcp_replay_truncated and registers nothing", async (t) => {
  const events = Array.from({ length: 5000 }, (_, i) => attached(10 + i, `server-${i}`, A));
  const f = ledger(t, events);
  await start(f);
  await f.quiet();
  const errors = adapterErrors(f);
  assert.equal(errors.length, 1);
  assert.equal(errors[0]!.code, "mcp_replay_truncated");
  assert.deepEqual(f.pi.registeredMcp, []);
});

test("manager and worker adapters replay at start as a lead does", async (t) => {
  for (const role of ["manager", "worker"] as const) {
    const f = ledger(t, [attached(10, "slack", A)], role);
    await start(f);
    assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: A }], role);
    assert.deepEqual(f.pi.sent, [], `${role}: no message and no turn`);
    await f.close();
  }
});
