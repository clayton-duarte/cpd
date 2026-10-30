import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, ID, planShow, shown } from "./fake.ts";

// commands.md §15.1 "Errors", §10.18, contract §1.2: a refused MCP registration is reported with `cpd session report` (one `error` Event, what `mcp attach <name>`)
// for every role, and is never deduplicated. Each test is red now: only a Lead reports, and `what` is `mcp attach` for every server.

const SERVER = { url: "https://mcp.slack.com/mcp", exposure: "hidden", toolExposure: { slack_read_channel: "direct" } };
const attached = (id: number, name = "slack") => ev(id, "mcp-attached", { session: ID, data: { name, server: SERVER } });

type Role = "lead" | "manager" | "worker";
const SHOWN: Record<Role, Record<string, unknown>> = {
  lead: {},
  manager: { role: "manager", plan: null, plan_lead: false },
  worker: { role: "worker", plan_lead: false },
};
const marker = (role: Role) => (id: number) =>
  role === "worker"
    ? ev(id, "attempt-nudge", { session: ID, data: { prompt: "Submit your work." } })
    : ev(id, "session-message", { session: ID, plan: role === "manager" ? null : "p1", text: "ship it", data: { from: null, text: "ship it", wake: false } });

async function started(t: { after(fn: () => Promise<void>): void }, role: Role, f = new Fixture()): Promise<Fixture> {
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown(SHOWN[role]));
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  f.emit(ev(101, "job-added")); // the cursor starts at 100: `drain` needs a higher id
  return f;
}

const CHAT = /adapter: mcp attach slack: mcp_register_failed: /;

for (const role of ["manager", "worker"] as const) {
  test(`a ${role} whose registration is refused reports one error Event and still shows the chat message`, async (t) => {
    const f = await started(t, role);
    f.pi.mcpThrows = new Error("pi refuses: bad server");
    f.emit(attached(200));
    await f.drain(marker(role));
    await f.quiet();
    const reports = f.reports();
    assert.equal(reports.length, 1, "exactly one report");
    assert.equal(reports[0]!.code, "mcp_register_failed");
    assert.equal(reports[0]!.what, "mcp attach slack");
    assert.equal(reports[0]!.session, ID);
    assert.equal(reports[0]!.details?.name, "slack");
    assert.equal(reports[0]!.details?.message, "pi refuses: bad server");
    const errors = f.pi.errors();
    assert.equal(errors.length, 1, "the one chat message");
    assert.match(errors[0]!.message.content, CHAT);
    assert.deepEqual(errors[0]!.options, { triggerTurn: false, deliverAs: "followUp" }, "no turn");
  });
}

test("two servers refused in a row are two reports naming the server in what", async (t) => {
  const f = await started(t, "lead");
  f.pi.mcpThrows = new Error("pi refuses");
  f.emit(attached(200, "slack"), attached(201, "atlassian"));
  await f.drain(marker("lead"));
  await f.quiet();
  assert.deepEqual(f.reports().map((r) => r.what), ["mcp attach slack", "mcp attach atlassian"]);
  f.emit(attached(300, "slack"));
  await f.drain(marker("lead"));
  await f.quiet();
  assert.deepEqual(f.reports().map((r) => r.what), ["mcp attach slack", "mcp attach atlassian", "mcp attach slack"]);
});

test("the same server refused twice in a row is two reports and two chat messages", async (t) => {
  const f = await started(t, "manager");
  f.pi.mcpThrows = new Error("pi refuses");
  f.emit(attached(200), attached(201));
  await f.drain(marker("manager"));
  await f.quiet();
  assert.deepEqual(f.reports().map((r) => r.what), ["mcp attach slack", "mcp attach slack"]);
  const errors = f.pi.errors();
  assert.equal(errors.length, 2, "two chat messages");
  for (const e of errors) assert.match(e.message.content, CHAT);

  // the control: another error of the same what and code twice in a row is shown once
  const before = f.pi.errors().length;
  f.emit("this is not json");
  await f.drain(marker("manager"));
  f.emit("this is not json either");
  await f.drain(marker("manager"));
  await f.quiet();
  assert.equal(f.pi.errors().length, before + 1, "the dedupe of every other error is unchanged");
});

test("a refused registration at start reaches the ledger as a report", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown(SHOWN.manager));
  const events = [attached(10, "slack"), attached(11, "atlassian")];
  f.respond("events-list@mcp-attached", { ok: true, events, next: events.length });
  f.pi.mcpThrowsFor.add("slack");
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  await f.quiet();
  assert.deepEqual(f.reports().map((r) => r.what), ["mcp attach slack"]);
  assert.deepEqual(f.pi.registeredMcp.map((r: { name: string }) => r.name), ["atlassian"]);
});

test("a failed report of a refused registration is one log line and no further report", async (t) => {
  const f = new Fixture();
  f.respond("session-report", { ok: false, error: { code: "store_busy", message: "database is locked", details: {} } }, { exit: 5 });
  await started(t, "manager", f);
  f.pi.mcpThrows = new Error("pi refuses");
  f.emit(attached(200));
  await f.drain(marker("manager"));
  await f.quiet();
  const failed = f.adapterLog().filter((l) => / report failed: /.test(l));
  assert.equal(failed.length, 1, f.adapterLog().join("\n"));
  assert.match(failed[0]!, /report failed: store_busy: database is locked$/);
  assert.equal(f.reports().length, 1, "no retry");
});
