import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { ENTRY_TYPE } from "../lib/loop-guard.ts";
import { ev, Fixture, ID, OTHER, sessionShow, stateCalls } from "./fake.ts";
import { CONFIG, withHost } from "./durable-host/rig.ts";

const LABEL = "blocked:φ: loop guard";
const GATED = { triggerTurn: false, deliverAs: "nextTurn" };
const tripEntries = (f: Fixture) => f.pi.entries
  .filter((e: any) => e.type === "custom" && e.customType === ENTRY_TYPE)
  .map((e: any) => e.data.tripped);
const clearWrites = (written: unknown[]) => written.filter((w) => {
  const entry = (w as { entry?: { kind?: string; data?: { tripped?: boolean } } }).entry;
  return entry?.kind === ENTRY_TYPE && entry.data?.tripped === false;
});

// The authenticated WebUI ingress and its outcome interface are unresolved (contract §6.13, §6.16).
// A client-controlled WebUI label is not that ingress. Lead decision webui-guard-tests-a1-q4:
// fail first for the existing socket bypass without inventing a credential or a positive recovery API.
test("webui_human_input_restores_tripped_guard", { timeout: 20_000 }, async (t) => {
  await t.test("TUI recovery and untrusted Event controls", async () => {
    const f = new Fixture();
    try {
      f.respond("session-show", sessionShow({ loop_guard_turns: 1 }));
      f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
      f.load({ now: () => 1_000_000 });
      await f.pi.fire("session_start");
      await f.followerStarted();
      for (let i = 0; i < 2; i++) await f.pi.fire("before_agent_start", { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } });
      await f.quiet();
      assert.ok(stateCalls(f).includes(LABEL));
      assert.deepEqual(tripEntries(f), [true]);

      f.emit(
        ev(301, "session-message", { session: ID, data: { from: null, text: "senderless", origin: "webui" } }),
        ev(302, "session-message", { session: ID, data: { from: OTHER, text: "agent", origin: "webui" } }),
        ev(303, "session-message", { session: OTHER, data: { from: null, text: "other target", origin: "webui" } }),
      );
      await f.drain();
      const delivered = f.pi.delivered();
      assert.deepEqual(delivered.map((s) => (s.message.details as any).id), [301, 302]);
      for (const s of delivered) {
        assert.equal((s.message.details as any).delivered_to, ID);
        assert.deepEqual(s.options, GATED, "receipt is held, not a turn or a reset");
      }
      assert.deepEqual(tripEntries(f), [true], "senderless and agent Events cannot clear the trip");
      assert.equal(stateCalls(f).at(-1), LABEL);

      await f.pi.fire("before_agent_start", { messageType: "user", prompt: "continue" });
      await f.pi.fire("agent_start");
      await f.quiet();
      assert.equal(stateCalls(f).at(-1), "working");
      assert.deepEqual(tripEntries(f), [true, false], "the actual local User prompt clears the trip");
      f.emit(ev(305, "job-failed"));
      await f.drain();
      const probe = f.pi.delivered().find((s) => (s.message.details as any).id === 305);
      assert.deepEqual(probe?.options, { triggerTurn: true, deliverAs: "followUp" });
    } finally {
      await f.close();
    }
  });

  await t.test("queued socket acknowledgement is not consumption", async () => {
    await withHost({}, async (r) => {
      await r.up();
      const c = r.connect();
      c.send({ type: "hello", protocol: 2, min_protocol: 2, client: "cpd-web" });
      c.send({ type: "attach", req: "attach" });
      await r.drain();
      assert.equal(c.received().find((m) => m.type === "welcome")?.session, CONFIG.session);
      c.send({ type: "submit", req: "wire-1", request_id: "request-1", text: "test input", when_busy: "followUp" });
      await r.drain();
      assert.deepEqual(c.received().find((m) => m.type === "result" && m.req === "wire-1"),
        { type: "result", req: "wire-1", data: { submission_id: 41, status: "queued" } });
      assert.equal(r.submitted[0]?.requestId, "chat:request-1");
      assert.deepEqual(r.submitted[0]?.entry, { kind: "cpd-chat", data: { client_id: "c1", request_id: "request-1" } });
      assert.equal(r.finish.length, 1, "the fake submission is still unsettled after the queued acknowledgement");
      assert.equal(r.written.some((w) => (w as { entry?: { kind?: string } }).entry?.kind === "cpd-chat"), false,
        "an acknowledgement is not a correlated transcript consumption entry");
      r.finish.splice(0).forEach((finish) => finish("done"));
      await r.drain();
    });
  });

  await t.test("unverified WebUI-labelled socket input cannot clear a trip", async () => {
    await withHost({ stored: [{ kind: ENTRY_TYPE, data: { tripped: true, at: 1, turns: 21 } }], answers: {} }, async (r) => {
      await r.up();
      const c = r.connect();
      c.send({ type: "hello", protocol: 2, min_protocol: 2, client: "cpd-web" });
      c.send({ type: "attach", req: "attach" });
      await r.drain();
      assert.equal(c.received().find((m) => m.type === "welcome")?.session, CONFIG.session);
      assert.ok(r.stateCalls().some((a) => a[2] === "blocked" && a.includes("φ: loop guard")), "the persisted trip is reported blocked");
      c.send({ type: "submit", req: "wire-2", request_id: "request-2", text: "unverified", when_busy: "followUp" });
      const response = await r.until(() => c.received().find((m) => m.req === "wire-2" && (m.type === "result" || m.type === "error")));
      assert.equal(clearWrites(r.written).length, 0, "a self-asserted WebUI label is not authenticated human input and must not clear the trip");
      assert.equal(r.submitted.length, 0, "without verified human input, the tripped guard must not start a turn");
      if (response.type === "result") assert.deepEqual(response.data, { submission_id: -1, status: "held" });
    });
  });
});

const TOKEN = "t".repeat(64);
const TRIPPED = [{ kind: ENTRY_TYPE, data: { tripped: true, at: 1, turns: 21 } }];

test("webui_verified_input_clears_trip_and_starts_turn", { timeout: 20_000 }, async () => {
  const home = mkdtempSync(join(tmpdir(), "webui-guard-"));
  try {
    mkdirSync(join(home, "run"));
    writeFileSync(join(home, "run", "webui-token"), TOKEN, { mode: 0o600 });
    await withHost({ stored: TRIPPED, answers: {}, config: { ...CONFIG, home } }, async (r) => {
      await r.up();
      const hello = (c: ReturnType<typeof r.connect>, token?: string) => {
        c.send({ type: "hello", protocol: 2, min_protocol: 2, client: "cpd-web", ...(token === undefined ? {} : { webui_token: token }) });
        c.send({ type: "attach", req: "attach" });
      };
      const wrong = r.connect();
      hello(wrong, "w".repeat(64));
      await r.drain();
      wrong.send({ type: "submit", req: "w1", request_id: "rw", text: "wrong token", when_busy: "followUp" });
      await r.until(() => wrong.received().find((m) => m.req === "w1" && (m.type === "result" || m.type === "error")));
      assert.equal(clearWrites(r.written).length, 0, "a wrong token never clears the trip");
      assert.equal(r.submitted.length, 0, "a wrong token starts no turn");

      const good = r.connect();
      hello(good, TOKEN);
      await r.drain();
      good.send({ type: "submit", req: "g1", request_id: "rg", text: "verified", when_busy: "followUp" });
      await r.until(() => good.received().find((m) => m.req === "g1" && (m.type === "result" || m.type === "error")));
      assert.equal(clearWrites(r.written).length, 1, "a valid token clears the trip");
      assert.equal(r.submitted.length, 1, "the verified input starts a turn");
      assert.equal(r.submitted[0]?.requestId, "chat:rg");
      const everything = JSON.stringify([r.written, r.submitted, r.logs, r.calls]);
      assert.equal(everything.includes(TOKEN), false, "the token is never stored, written or logged");
      r.finish.splice(0).forEach((finish) => finish("done"));
      await r.drain();
    });
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
});

test("lead_trip_reports_actual_guard_kind_and_count", { timeout: 20_000 }, async () => {
  const f = new Fixture();
  try {
    f.respond("session-show", sessionShow({ loop_guard_turns: 1 }));
    f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
    f.load({ now: () => 1_000_000 });
    await f.pi.fire("session_start");
    await f.followerStarted();
    for (let i = 0; i < 2; i++) await f.pi.fire("before_agent_start", { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } });
    await f.quiet();
    assert.ok(stateCalls(f).includes(LABEL));
    const trips = f.reports().filter((x) => x.details?.kind === "inter-turn");
    assert.equal(trips.length, 1, "one trip report with its guard kind");
    assert.equal(trips[0]!.details!.count, 2, "count is loop_guard_turns + 1");
    assert.match(trips[0]!.message ?? "", /loop guard, 2 turns in 10 min/);
  } finally {
    await f.close();
  }
});

test("lead_trip_wakes_manager", { timeout: 20_000 }, async () => {
  const f = new Fixture();
  try {
    f.respond("session-show", sessionShow({ loop_guard_turns: 20 }));
    f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
    f.load({ now: () => 1_000_000 });
    await f.pi.fire("session_start");
    await f.followerStarted();
    await f.pi.fire("before_agent_start", { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } });
    let blocked = 0;
    for (let i = 0; i < 15; i++) {
      const [verdict] = (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: `r${i}`, input: { command: "rg foo" } })) as { block?: boolean }[];
      if (verdict?.block) blocked++;
    }
    assert.equal(blocked, 3, "calls 13 to 15 are refused");
    await f.quiet();
    assert.ok(stateCalls(f).includes(LABEL));
    const trips = f.reports().filter((x) => x.details?.kind === "turn-refusals");
    assert.equal(trips.length, 1, "one report (one transaction) carries the Event and the Manager's wake");
    assert.equal(trips[0]!.details!.count, 3);
    assert.match(trips[0]!.message ?? "", new RegExp(`tripped: loop guard, 3 refused calls in one turn\\. It waits for the User's message or for cpd session resume ${ID}\\.`));
  } finally {
    await f.close();
  }
});
