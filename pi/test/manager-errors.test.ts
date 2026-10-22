import assert from "node:assert/strict";
import { test } from "node:test";
import { PROTOCOL } from "../lib/event.ts";
import { ev, Fixture, ID, OTHER, planShow, shown, waitFor } from "./fake.ts";

/**
 * commands.md §15.1 "Errors" and §10.18 (hs-d573, cause A): a Lead's adapter errors are written to the adapter log and reported to the Manager with
 * `cpd session report`, once per (what, code); they are never delivered to the Lead's chat and never notified. A Manager's and a Worker's are still delivered with no turn.
 */

type Role = "lead" | "fork" | "manager" | "worker";

const SHOWN: Record<Role, Record<string, unknown>> = {
  lead: {},
  fork: { plan_lead: false }, // a side fork: role lead, not the Plan's Lead
  manager: { role: "manager", plan: null, plan_lead: false },
  worker: { role: "worker", plan_lead: false },
};

/** The last Event of a `drain`: one that the Session of `role` always delivers. */
const marker = (role: Role) => (id: number) =>
  role === "worker"
    ? ev(id, "attempt-nudge", { session: ID, data: { prompt: "Submit your work." } })
    : ev(id, "session-message", { session: ID, text: "ship it", data: { from: null, text: "ship it", wake: false } });

async function started(f: Fixture, role: Role) {
  f.respond("plan-show", planShow());
  f.respond("session-show", shown(SHOWN[role]));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(ev(101, "job-added")); // the cursor starts at 100 (registered_event): `drain` needs a higher id than that
}

/** The adapter error `events follow: invalid_event`: a line that is not an Event. */
async function badLine(f: Fixture, role: Role) {
  f.emit("this is not json");
  await f.drain(marker(role));
}

/** The adapter error `session state: illegal_transition`: a refused state call. */
async function refusedState(f: Fixture) {
  f.respond("session-state", { ok: false, error: { code: "illegal_transition", message: "stopped to working.", details: {} } }, { exit: 3 });
  await f.pi.fire("agent_start");
}

const reported = (f: Fixture, n: number) => waitFor(`${n} report(s)`, () => f.reports().length >= n, 3000);
const lines = (f: Fixture) => f.adapterLog().map((l) => l.replace(/^\S+ /, "")); // without the time

test("a Lead's adapter error is not delivered to its chat and notifies nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  await badLine(f, "lead");
  await refusedState(f);
  await f.quiet();
  assert.deepEqual(f.pi.sent, [], "no message: no turn, no follow-up, nothing in the chat");
  assert.deepEqual(f.pi.notices, []);
  assert.deepEqual(f.pi.errorNotices, []);
  await reported(f, 2); // and the errors went somewhere
});

test("a Lead's adapter error is reported with session report once per what and code", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit("this is not json", "this is not json either");
  await f.drain(marker("lead"));
  await reported(f, 1);
  await f.quiet();
  assert.equal(f.reports().length, 1, "the same what and code twice is one report");
  const [r] = f.reports();
  assert.deepEqual(r!.argv.slice(0, 7), ["session", "report", "--what", "events follow", "--code", "invalid_event", "--message"]);
  assert.match(r!.message ?? "", /not a valid Event/);
  assert.equal(r!.session, ID);

  await refusedState(f);
  await reported(f, 2);
  assert.deepEqual(f.reports().map((x) => [x.what, x.code, x.message]), [
    ["events follow", "invalid_event", r!.message],
    ["session state", "illegal_transition", "stopped to working."],
  ], "a different what and code is reported");

  await badLine(f, "lead");
  await reported(f, 3);
  await f.quiet();
  assert.deepEqual(f.reports().map((x) => [x.what, x.code]), [["events follow", "invalid_event"], ["session state", "illegal_transition"], ["events follow", "invalid_event"]], "the first again, after a different one");
});

test("a Lead's adapter error is written to the adapter log", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit("this is not json", "this is not json either");
  await f.drain(marker("lead"));
  await reported(f, 1);
  await f.quiet();
  const errors = f.adapterLog().filter((l) => / error: /.test(l));
  assert.equal(errors.length, 1, "once per what and code, as the report is");
  assert.match(errors[0]!, /^\d{4}-\d\d-\d\dT\S+ error: events follow: invalid_event: .*not a valid Event/);

  await refusedState(f);
  await reported(f, 2);
  await f.quiet();
  assert.deepEqual(lines(f).filter((l) => l.startsWith("error: ")).map((l) => l.split(": ").slice(0, 3).join(": ")), ["error: events follow: invalid_event", "error: session state: illegal_transition"]);
  assert.ok(lines(f).includes("error: session state: illegal_transition: stopped to working."));
});

test("a failed report is logged and not delivered", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-report", { ok: false, error: { code: "store_busy", message: "database is locked", details: {} } }, { exit: 5 });
  await started(f, "lead");
  await badLine(f, "lead");
  await reported(f, 1);
  await f.quiet();
  assert.ok(lines(f).includes("report failed: store_busy: database is locked"), lines(f).join("\n"));
  assert.ok(lines(f).some((l) => l.startsWith("error: events follow: invalid_event: ")), "the error itself is logged too");
  assert.deepEqual(f.pi.sent, [], "never delivered");
  assert.deepEqual(f.pi.notices, []);

  await badLine(f, "lead");
  await f.quiet();
  assert.equal(f.reports().length, 1, "the same error is not reported again, however the report went");

  await refusedState(f);
  await reported(f, 2);
  await f.quiet();
  assert.deepEqual(f.reports().map((x) => x.code), ["invalid_event", "illegal_transition"], "the next different error tries again");
  assert.deepEqual(f.pi.sent, [], "the report that failed is not an error to report (no recursion)");
  assert.equal(f.reports().length, 2);
});

test("a side fork's adapter error goes the Lead's way", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "fork");
  await badLine(f, "fork");
  await reported(f, 1);
  await f.quiet();
  assert.deepEqual(f.pi.sent, []);
  assert.deepEqual(f.reports().map((x) => [x.what, x.code, x.session]), [["events follow", "invalid_event", ID]]);
  assert.ok(lines(f).some((l) => l.startsWith("error: events follow: invalid_event: ")));
});

/** What the Session of `role` is shown and reports for the bad line. */
async function outcome(role: Role) {
  const f = new Fixture();
  try {
    await started(f, role);
    await badLine(f, role);
    await f.quiet();
    return { errors: f.pi.errors(), reports: f.reports() };
  } finally {
    await f.close();
  }
}

test("a Manager's adapter error is still delivered with no turn", async () => {
  const manager = await outcome("manager");
  assert.equal(manager.errors.length, 1);
  assert.match(manager.errors[0]!.message.content, /^cpd adapter: events follow: invalid_event: /);
  assert.deepEqual(manager.errors[0]!.options, { triggerTurn: false, deliverAs: "followUp" });
  assert.deepEqual(manager.reports, [], "a Manager reports to no one");
  // the role decides: the same error of a Lead is reported and not delivered
  const lead = await outcome("lead");
  assert.deepEqual(lead.errors, []);
  assert.equal(lead.reports.length, 1);
});

test("a Worker's adapter error is still delivered with no turn", async () => {
  const worker = await outcome("worker");
  assert.equal(worker.errors.length, 1);
  assert.match(worker.errors[0]!.message.content, /^cpd adapter: events follow: invalid_event: /);
  assert.deepEqual(worker.errors[0]!.options, { triggerTurn: false, deliverAs: "followUp" });
  assert.deepEqual(worker.reports, [], "a Worker reports to no one");
  const lead = await outcome("lead");
  assert.deepEqual(lead.errors, []);
  assert.equal(lead.reports.length, 1);
});

// ---- every path that raises an adapter error for a Lead ----

type Path = { name: string; what: string; code: string; trigger(f: Fixture): Promise<void> };
const busy = { ok: false, error: { code: "store_busy", message: "database is locked", details: {} } };
const beforeAgentStart = (f: Fixture) => f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" });

const PATHS: Path[] = [
  { name: "host.deliver (a failed call)", what: "session state", code: "illegal_transition", trigger: (f) => refusedState(f) },
  { name: "host.deliver (a line that is no Event)", what: "events follow", code: "invalid_event", trigger: (f) => badLine(f, "lead") },
  {
    name: "the first-message failure",
    what: "first message",
    code: "store_busy",
    trigger: async (f) => {
      f.respond("session-first-message", busy, { exit: 5 });
      await beforeAgentStart(f);
    },
  },
  {
    name: "the protocol mismatch",
    what: "protocol mismatch",
    code: "protocol_mismatch",
    trigger: async () => {}, // at start: see `lead`
  },
  {
    name: "a failed read of Context updates",
    what: "context updates",
    code: "store_busy",
    trigger: async (f) => {
      f.respond("events-list@context-record", busy, { exit: 5 });
      await beforeAgentStart(f);
    },
  },
];

test("every adapter error path of a Lead is reported, none delivered", async () => {
  for (const path of PATHS) {
    const f = new Fixture();
    try {
      f.respond("session-show", shown({ by_user: true }));
      f.respond("session-first-message", { ok: true, session: ID, role: "lead", text: "x", file: "/f", event: 120 });
      if (path.code === "protocol_mismatch") f.respond("--version", { ok: true, version: "0.0.0", schema: 6, store_schema: 6, protocol: PROTOCOL + 1 });
      f.respond("plan-show", planShow());
      f.load();
      await f.pi.fire("session_start");
      await waitFor("the follower", () => f.follows().length > 0);
      f.emit(ev(101, "job-added"));
      await path.trigger(f);
      await reported(f, 1).catch((e) => assert.fail(`${path.name}: not reported (${e.message})`));
      await f.quiet();
      assert.deepEqual(f.pi.sent, [], `${path.name}: nothing delivered`);
      assert.deepEqual(f.pi.notices, [], `${path.name}: nothing notified`);
      const [r, ...more] = f.reports();
      assert.deepEqual(more, [], `${path.name}: one report`);
      assert.deepEqual([r!.what, r!.code, r!.session], [path.what, path.code, ID], path.name);
      assert.ok((r!.message ?? "") !== "", `${path.name}: a message`);
      if (path.code === "protocol_mismatch") {
        assert.match(r!.message!, new RegExp(`cpd speaks protocol ${PROTOCOL + 1}, this adapter ${PROTOCOL}; reload pi \\(/reload\\)`));
        assert.equal(r!.details?.cpd, PROTOCOL + 1);
        assert.equal(r!.details?.adapter, PROTOCOL);
      }
      assert.ok(lines(f).some((l) => l.startsWith(`error: ${path.what}: ${path.code}: `)), `${path.name}: logged`);
    } finally {
      await f.close();
    }
  }

  // the activation mismatch: CPD_SESSION is not pi's session. A Lead's pane has CPD_PLAN (§6.5), and `session show` would say lead too.
  const f = new Fixture();
  try {
    process.env.CPD_PLAN = "p1";
    f.load();
    f.pi.sessionId = OTHER;
    await f.pi.fire("session_start");
    await reported(f, 1).catch((e) => assert.fail(`the activation mismatch: not reported (${e.message})`));
    await f.quiet();
    assert.deepEqual(f.pi.sent, [], "activation: nothing delivered");
    assert.deepEqual(f.pi.notices, []);
    const [r, ...more] = f.reports();
    assert.deepEqual(more, []);
    assert.deepEqual([r!.what, r!.code, r!.session], ["activation", "session_mismatch", ID]);
    assert.ok(lines(f).some((l) => l.startsWith("error: activation: session_mismatch: ")), "activation: logged");
  } finally {
    await f.close();
  }
});
