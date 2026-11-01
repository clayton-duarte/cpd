import assert from "node:assert/strict";
import { test } from "node:test";
import { sessionMessage } from "../../lib/delivery.ts";
import type { CpdEvent, EventOf } from "../../lib/event.ts";
import { ev } from "../fake.ts";
import { inputOf } from "../../durable-host/deliver.ts";

const event = (id: number, kind: string, over: Record<string, unknown> = {}) => ev(id, kind, { session: "s1", ...over }) as unknown as CpdEvent;
const LEAD = { role: "lead", planLead: true };
const entry = (e: CpdEvent) => ({ kind: "cpd-event", data: { id: e.id, kind: e.kind, delivered_to: "s1" } });

test("inputOf maps a session-message for the Session to a follow-up input that names the sender", () => {
  const user = event(7, "session-message", { data: { text: "hello", from: null } });
  assert.deepEqual(inputOf(user, "s1", LEAD), {
    content: sessionMessage(user as EventOf<"session-message">, "s1", null).content,
    requestId: "event:7",
    whenBusy: "followUp",
    entry: entry(user),
    reply: true,
  });
  const peer = event(8, "session-message", { data: { text: "hi", from: "s9", wake: false } });
  const input = inputOf(peer, "s1", LEAD);
  assert.deepEqual(input, {
    content: sessionMessage(peer as EventOf<"session-message">, "s1", { id: "s9", name: null }).content,
    requestId: "event:8",
    whenBusy: "followUp",
    entry: entry(peer),
  });
  assert.ok(input!.content.includes("s9") && input!.content.includes("hi"));
});

test("inputOf maps pr-feedback and escalation to steered inputs", () => {
  for (const kind of ["pr-feedback", "escalation"]) {
    const e = event(11, kind, { data: { escalation: "e1" } });
    assert.deepEqual(inputOf(e, "s1", LEAD), { content: e.text, requestId: "event:11", whenBusy: "steer", entry: entry(e) }, kind);
  }
});

test("inputOf maps job-failed, plan-done, error and job-merged with data.turn to follow-up inputs", () => {
  for (const kind of ["job-failed", "plan-done", "error"]) {
    const e = event(12, kind);
    assert.deepEqual(inputOf(e, "s1", LEAD), { content: e.text, requestId: "event:12", whenBusy: "followUp", entry: entry(e) }, kind);
  }
  const merged = event(13, "job-merged", { data: { turn: true } });
  assert.deepEqual(inputOf(merged, "s1", LEAD), { content: merged.text, requestId: "event:13", whenBusy: "followUp", entry: entry(merged) });
});

test("inputOf maps job-merged without data.turn to nothing", () => {
  assert.equal(inputOf(event(14, "job-merged", { data: {} }), "s1", LEAD), undefined);
  assert.equal(inputOf(event(15, "job-merged", { data: { turn: false } }), "s1", LEAD), undefined);
});

test("inputOf maps the kinds that add nothing to nothing", () => {
  for (const kind of ["attempt-retry", "job-cancelled", "workflow-started", "workflow-base-changed", "pr-merged", "all-merged", "attempt-nudge"]) {
    assert.equal(inputOf(event(16, kind, { data: { prompt: "x" } }), "s1", LEAD), undefined, kind);
  }
});

test("inputOf maps an Event of another Session to nothing", () => {
  assert.equal(inputOf(event(17, "session-message", { session: "s2", data: { text: "x", from: null } }), "s1", LEAD), undefined);
  assert.equal(inputOf(event(18, "session-message", { session: null, data: { text: "x", from: null } }), "s1", LEAD), undefined);
});

test("inputOf maps session-resumed, session-retiered, mcp-attached and session-renamed to nothing", () => {
  for (const kind of ["session-resumed", "session-retiered", "mcp-attached", "session-renamed", "plan-review", "escalation-answered", "escalation-expired"]) {
    assert.equal(inputOf(event(19, kind, { data: { name: "n", server: {}, review: "r", escalation: "e1", status: "pending" } }), "s1", LEAD), undefined, kind);
  }
});

test("inputOf gives a Manager and a Lead with no Plan only session-message", () => {
  for (const info of [{ role: "manager", planLead: false }, { role: "lead", planLead: false }]) {
    for (const kind of ["job-merged", "job-failed", "plan-done", "error", "pr-feedback", "escalation"]) {
      assert.equal(inputOf(event(20, kind, { data: { turn: true } }), "s1", info), undefined, `${info.role} ${kind}`);
    }
    const m = event(21, "session-message", { data: { text: "hi", from: null } });
    assert.deepEqual(inputOf(m, "s1", info), {
      content: sessionMessage(m as EventOf<"session-message">, "s1", null).content,
      requestId: "event:21",
      whenBusy: "followUp",
      entry: entry(m),
      reply: true,
    });
  }
});

test("inputOf gives every input the requestId event:<id>", () => {
  const kinds: [string, Record<string, unknown>][] = [
    ["session-message", { text: "x", from: null }],
    ["job-merged", { turn: true }],
    ["job-failed", {}],
    ["plan-done", {}],
    ["error", {}],
    ["pr-feedback", {}],
    ["escalation", {}],
  ];
  kinds.forEach(([kind, data], i) => {
    const id = 100 + i * 7;
    assert.equal(inputOf(event(id, kind, { data }), "s1", LEAD)?.requestId, `event:${id}`, kind);
  });
});

test("inputOf marks a session-message from the User as a reply", () => {
  const input = inputOf(event(30, "session-message", { data: { text: "hello", from: null } }), "s1", LEAD);
  assert.equal(input?.reply, true);
});

test("inputOf does not mark a message from a Session, nor any other kind, as a reply", () => {
  const peer = inputOf(event(31, "session-message", { data: { text: "hi", from: "s9", wake: false } }), "s1", LEAD);
  assert.ok(peer);
  assert.equal("reply" in peer, false);
  assert.equal(inputOf(event(33, "session-message", { data: { text: "hello", from: null } }), "s1", LEAD)?.reply, true, "contrast: the User's message is a reply");
  for (const kind of ["pr-feedback", "escalation", "job-failed", "plan-done", "error"]) {
    const input = inputOf(event(32, kind), "s1", LEAD);
    assert.ok(input, kind);
    assert.equal("reply" in input, false, kind);
  }
});

test("inputOf gives every input an entry of kind cpd-event", () => {
  const kinds: [string, Record<string, unknown>][] = [
    ["session-message", { text: "x", from: null }],
    ["session-message", { text: "x", from: "s9" }],
    ["job-merged", { turn: true }],
    ["job-failed", {}],
    ["pr-feedback", {}],
    ["escalation", {}],
  ];
  kinds.forEach(([kind, data], i) => {
    const e = event(40 + i, kind, { data });
    assert.deepEqual(inputOf(e, "s1", LEAD)?.entry, entry(e), kind);
  });
  const user = event(60, "session-message", { data: { text: "x", from: null } });
  assert.deepEqual(inputOf(user, "s1", LEAD), { ...inputOf(user, "s1", LEAD), entry: entry(user), reply: true }, "the entry is kept beside the reply flag");
});
