import assert from "node:assert/strict";
import { test } from "node:test";
import { cursorFrom } from "../../durable-host/cursor.ts";

const delivered = (id: number, to: string) => ({ kind: "cpd-event", data: { id, kind: "job-failed", delivered_to: to } });

test("the cursor is the highest delivered Event id of this Session among the stored entries", () => {
  // steered kinds make delivery out of id order: the highest id, not the last
  const entries = [delivered(12, "s1"), delivered(40, "s1"), { kind: "pi.user", model: [] }, delivered(30, "s1")];
  assert.equal(cursorFrom(0, entries, "s1"), 40);
  assert.equal(
    cursorFrom(0, [{ kind: "cpd-first-message", data: { event: 55, delivered_to: "s1" } }, { kind: "cpd-context", data: { event: 61, delivered_to: "s1" } }, delivered(20, "s1")], "s1"),
    61,
  );
});

test("an entry delivered to another Session does not move the cursor", () => {
  const entries = [delivered(12, "s1"), delivered(90, "source"), { kind: "cpd-context", data: { event: 95, delivered_to: "source" } }];
  assert.equal(cursorFrom(0, entries, "s1"), 12);
});

test("the cursor is the greater of registered_event and the stored entries", () => {
  assert.equal(cursorFrom(100, [delivered(12, "s1")], "s1"), 100);
  assert.equal(cursorFrom(5, [delivered(12, "s1")], "s1"), 12);
  assert.equal(cursorFrom(7, [], "s1"), 7);
});
