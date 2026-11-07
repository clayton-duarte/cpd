import assert from "node:assert/strict";
import { test } from "node:test";
import { parseLine } from "../lib/event.ts";

/** A well-formed `session-message` line with `data` as given (contract §5.2, Plan ui-origin). */
const line = (data: Record<string, unknown>): string =>
  JSON.stringify({
    id: 7,
    at: "t",
    kind: "session-message",
    context: null,
    plan: null,
    job: null,
    attempt: null,
    session: "s",
    text: "x",
    data,
  });

test("a session-message event with origin parses it", () => {
  for (const origin of ["tui", "webui"]) {
    const parsed = parseLine(line({ from: null, text: "hi", unblocks: "u", origin }));
    assert.equal(parsed.type, "event", `${origin} parses as an Event`);
    assert.equal((parsed as { event: { data: Record<string, unknown> } }).event.data.origin, origin);
  }
  // The value is exactly tui|webui: any other is refused at the boundary.
  assert.equal(
    parseLine(line({ from: null, text: "hi", unblocks: "u", origin: "bogus" })).type,
    "invalid",
    "an unknown origin is invalid",
  );
});

test("a session-message event without origin parses", () => {
  const parsed = parseLine(line({ from: null, text: "hi", unblocks: "u" }));
  assert.equal(parsed.type, "event", "an absent origin is valid");
  assert.ok(
    !("origin" in (parsed as { event: { data: Record<string, unknown> } }).event.data),
    "an absent origin is no key",
  );
  // Present but not tui|webui (null included) is refused: absent means no origin, not null.
  assert.equal(
    parseLine(line({ from: null, text: "hi", unblocks: "u", origin: null })).type,
    "invalid",
    "a null origin is invalid",
  );
});
