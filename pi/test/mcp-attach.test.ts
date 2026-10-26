import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, ID, OTHER, planShow, shown } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

// pro-d8, commands.md §15.3 `mcp-attached`, contract §5.3 and §6.11: the Event for the adapter's own Session registers the server with pi (`pi.registerMcpServer(name, server)`),
// with no message and no turn; one for another Session registers nothing. Each test is red now because the adapter does not know the kind.

const SERVER = { url: "https://mcp.slack.com/mcp", exposure: "hidden", toolExposure: { slack_read_channel: "direct" } };
const attached = (id: number, session: string = ID, over: Record<string, unknown> = {}) =>
  ev(id, "mcp-attached", { session, data: { name: "slack", server: SERVER, ...over } });

type Role = "lead" | "manager" | "worker";
const SHOWN: Record<Role, Record<string, unknown>> = {
  lead: {},
  manager: { role: "manager", plan: null, plan_lead: false },
  worker: { role: "worker", plan_lead: false },
};
/** The last Event of a `drain`: one that the Session of `role` always delivers (as manager-errors.test.ts). */
const marker = (role: Role) => (id: number) =>
  role === "worker"
    ? ev(id, "attempt-nudge", { session: ID, data: { prompt: "Submit your work." } })
    : ev(id, "session-message", { session: ID, plan: role === "manager" ? null : "p1", text: "ship it", data: { from: null, text: "ship it", wake: false } });

async function started(t: { after(fn: () => Promise<void>): void }, role: Role = "lead"): Promise<Fixture> {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown(SHOWN[role]));
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  f.emit(ev(101, "job-added")); // the cursor starts at 100: `drain` needs a higher id
  return f;
}

test("mcp-attached for its own Session registers the server under its name with the Event's config, and shows no message", async (t) => {
  const f = await started(t);
  f.emit(attached(200));
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: SERVER }]);
  assert.deepEqual(f.pi.sent, [], "no message and no turn");
  assert.deepEqual(f.pi.errors(), []);
});

test("mcp-attached for another Session registers nothing", async (t) => {
  const f = await started(t);
  f.emit(attached(200, OTHER));
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp, [], "the other Session's Event registers nothing");
  assert.deepEqual(f.pi.sent, []);
  f.emit(attached(300)); // the control: the same Event for its own Session is the one that registers
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: SERVER }]);
});

test("mcp-attached handled twice registers the same name twice with equal config", async (t) => {
  const f = await started(t);
  f.emit(attached(200), attached(201));
  await f.drain();
  assert.equal(f.pi.registeredMcp.length, 2);
  assert.equal(f.pi.registeredMcp[0]!.name, "slack");
  assert.equal(f.pi.registeredMcp[1]!.name, "slack");
  assert.deepEqual(f.pi.registeredMcp[1]!.config, f.pi.registeredMcp[0]!.config);
  assert.deepEqual(f.pi.registeredMcp[0]!.config, SERVER);
});

test("a registerMcpServer that throws is reported once as mcp_register_failed with the name and pi's message", async (t) => {
  const f = await started(t);
  f.pi.mcpThrows = new Error("pi refuses: bad server");
  f.emit(attached(200));
  await f.drain();
  const errors = adapterErrors(f);
  const e = errors[0]!;
  assert.equal(errors.length, 1, "reported once");
  assert.equal(e.code, "mcp_register_failed");
  assert.ok(e.message.content.includes("pi refuses: bad server"), `pi's message is in the report: ${e.message.content}`);
  const [report] = f.reports();
  assert.equal(report!.details?.name, "slack");
});

test("with no mcp-attached Event the adapter registers no server, whatever else it handles", async (t) => {
  const f = await started(t);
  f.emit(ev(200, "session-state", { session: ID, data: { state: "idle" } }), ev(201, "job-merged", { data: { turn: true } }));
  await f.drain();
  await f.pi.fire("agent_start");
  await f.pi.fire("agent_settled");
  await f.quiet();
  assert.deepEqual(f.pi.registeredMcp, []);
  f.emit(attached(300)); // the control: the adapter registers only when this Event comes
  await f.drain();
  assert.deepEqual(f.pi.registeredMcp.map((r: { name: string }) => r.name), ["slack"]);
});

test("mcp-attached registers the server in a Manager's and a Worker's adapter as in a Lead's", async (t) => {
  for (const role of ["manager", "worker"] as const) {
    const f = await started(t, role);
    f.emit(attached(200));
    await f.drain(marker(role));
    assert.deepEqual(f.pi.registeredMcp, [{ name: "slack", config: SERVER }], role);
    assert.deepEqual(f.pi.sent, [], `${role}: no message and no turn`);
    await f.close();
  }
});
