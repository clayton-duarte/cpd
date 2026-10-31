import assert from "node:assert/strict";
import { test } from "node:test";
import { KINDS } from "../lib/event.ts";
import { ev, Fixture, ID, planShow, shown, waitFor } from "./fake.ts";

// commands.md §15.3 `session-model-changed` (the tier-preset switch, §14.6): the same switch path as `session-retiered`'s —
// no message, no turn; at the Session's next idle the adapter reads `cpd session show` (`model`, `thinking`) and calls the mock
// pi's `setModel` and `setThinkingLevel` once. This test is red before the implementation: the adapter does not know the kind,
// so `KINDS` lacks it and the switch sets nothing.

/** Long enough for the adapter's one `cpd session show` and the calls after it. */
const WAIT = 5000;
const changed = (id: number, session: string = ID) => ev(id, "session-model-changed", { session, data: { from: "p/opus", to: "p/haiku" } });

/** The row the preset switch wrote: `p/haiku` at `low`. */
const shownAfter = (over: Record<string, unknown> = {}) => shown({ model: "p/haiku", thinking: "low", ...over });

async function started(t: { after(fn: () => Promise<void>): void }): Promise<Fixture> {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.respond("session-show", shownAfter()); // the row `preset apply` wrote
  return f;
}

test("a session-model-changed event is no message and switches the model at the next idle", async (t) => {
  assert.ok((KINDS as readonly string[]).includes("session-model-changed"), "session-model-changed is not in KINDS");
  const f = await started(t);
  f.emit(changed(600));
  await waitFor("setModel", () => f.pi.modelsSet.length > 0, WAIT);
  await f.drain();
  // the mock pi's setModel and setThinkingLevel ran once, at the idle, from what `session show` reported
  assert.deepEqual(f.pi.modelsSet, [{ provider: "p", id: "haiku" }]);
  assert.deepEqual(f.pi.thinkingSet, ["low"]);
  assert.deepEqual(f.pi.sent, [], "no message and no turn: the switch is silent");
});