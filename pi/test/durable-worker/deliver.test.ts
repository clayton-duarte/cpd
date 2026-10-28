import assert from "node:assert/strict";
import { test } from "node:test";
import { sessionMessage } from "../../lib/delivery.ts";
import type { CpdEvent, EventOf } from "../../lib/event.ts";
import { ev } from "../fake.ts";
import { inputOf } from "../../durable-worker/deliver.ts";

const event = (id: number, kind: string, over: Record<string, unknown>) => ev(id, kind, over) as unknown as CpdEvent;

test("an attempt-nudge for this Session becomes a follow-up input with the prompt and requestId event:<id>", () => {
  const e = event(41, "attempt-nudge", { session: "s1", data: { prompt: "Carry on." } });
  assert.deepEqual(inputOf(e, "s1"), { content: "Carry on.", requestId: "event:41", whenBusy: "followUp" });
});

test("a session-message for this Session becomes a follow-up input with the Message from text", () => {
  const user = event(7, "session-message", { session: "s1", data: { text: "hello", from: null } });
  assert.deepEqual(inputOf(user, "s1"), {
    content: sessionMessage(user as EventOf<"session-message">, "s1", null).content,
    requestId: "event:7",
    whenBusy: "followUp",
  });
  const peer = event(8, "session-message", { session: "s1", data: { text: "hi", from: "s9" } });
  assert.deepEqual(inputOf(peer, "s1"), {
    content: sessionMessage(peer as EventOf<"session-message">, "s1", { id: "s9", name: null }).content,
    requestId: "event:8",
    whenBusy: "followUp",
  });
});

test("an Event for another Session or of another kind becomes nothing", () => {
  assert.equal(inputOf(event(1, "attempt-nudge", { session: "s2", data: { prompt: "x" } }), "s1"), undefined);
  assert.equal(inputOf(event(2, "session-message", { session: "s2", data: { text: "x", from: null } }), "s1"), undefined);
  assert.equal(inputOf(event(3, "job-merged", { session: "s1" }), "s1"), undefined);
  assert.equal(inputOf(event(4, "attempt-nudge", { session: null, data: { prompt: "x" } }), "s1"), undefined);
});
