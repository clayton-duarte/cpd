import assert from "node:assert/strict";
import { test } from "node:test";
import { gated } from "../../lib/delivery.ts";
import type { Host } from "../../lib/host.ts";
import { clearTrip, ENTRY_TYPE, holdTrip, KEY, LABEL, trippedIn, WINDOW_MS } from "../../lib/loop-guard.ts";
import type { Run } from "../../lib/run.ts";
import { loopGate, type LoopRuntime } from "../../durable-host/run.ts";

/** The contract §6.16 seam: the pure loop gate over a fake clock, a fake runtime and a fake `facts`; nothing real runs. */
type Fake = {
  r: LoopRuntime & { facts: { open(key: string, label: string, kind: string): void; close(key: string): void } };
  host: Host;
  reasons: Map<string, string>;
  appended: { type: string; data: unknown }[];
};

function fake(over: { role?: "lead" | "manager"; bound?: number; turnStarts?: readonly number[]; tripped?: boolean } = {}): Fake {
  const reasons = new Map<string, string>();
  const appended: { type: string; data: unknown }[] = [];
  const r = {
    role: over.role ?? "lead",
    info: { loopGuardTurns: over.bound ?? 20 },
    turnStarts: [...(over.turnStarts ?? [])],
    tripped: over.tripped ?? false,
    facts: {
      open: (key: string, label: string) => {
        reasons.set(key, label);
      },
      close: (key: string) => {
        reasons.delete(key);
      },
    },
  };
  const host = {
    pi: {
      appendEntry: (type: string, data: unknown) => {
        appended.push({ type, data });
      },
    },
    refresh: () => {},
  } as unknown as Host;
  return { r, host, reasons, appended };
}

/** A fake persisted `cdp-loop-guard` entry, the pi session shape `trippedIn` reads. */
const entry = (tripped: boolean) => ({ type: "custom", customType: ENTRY_TYPE, data: { tripped } });

test("the loop guard trips after more than the bound self-started submissions in the window", () => {
  const f = fake({ bound: 3 });
  const now = 1_000_000;
  for (let i = 0; i < 3; i++) assert.equal(loopGate(f.r, now + i), false, `submission ${i + 1} is at the bound`);
  assert.equal(loopGate(f.r, now + 3), true, "the fourth is over the bound");
  assert.equal(f.r.turnStarts.length, 4);
  // a start outside the window is dropped, so the turns it fed no longer trip
  const later = now + WINDOW_MS + 10;
  const g = fake({ bound: 3, turnStarts: [now, now + 1, now + 2, now + 3] });
  assert.equal(loopGate(g.r, later), false);
  assert.deepEqual(g.r.turnStarts, [later]);
});

test("a tripped loop guard holds the next delivery and reports blocked with the loop guard label", () => {
  const f = fake({ bound: 1, turnStarts: [1_000, 2_000] });
  assert.equal(loopGate(f.r, 3_000), true);
  holdTrip(f.r as unknown as Run);
  assert.equal(f.r.tripped, true);
  assert.equal(f.reasons.get(KEY), LABEL);
  assert.equal(LABEL, "φ: loop guard");
  // the held delivery: a send that would start a turn is held for the next turn
  assert.deepEqual(gated({ firstPending: false, tripped: f.r.tripped }, { triggerTurn: true, deliverAs: "followUp" }), { triggerTurn: false, deliverAs: "nextTurn" });
});

test("a resume clears the trip and the held delivery goes through", () => {
  const f = fake({ bound: 1, turnStarts: [1_000, 2_000], tripped: true });
  f.reasons.set(KEY, LABEL);
  // before the resume the delivery is held
  assert.equal(gated({ firstPending: false, tripped: true }, { triggerTurn: true, deliverAs: "followUp" }).triggerTurn, false);
  assert.equal(clearTrip(f.host, f.r as unknown as Run), true);
  assert.equal(f.r.tripped, false);
  assert.deepEqual(f.r.turnStarts, []);
  assert.equal(f.reasons.has(KEY), false);
  assert.deepEqual(f.appended, [{ type: ENTRY_TYPE, data: { tripped: false } }]);
  // the held delivery goes through now
  assert.equal(gated({ firstPending: false, tripped: f.r.tripped }, { triggerTurn: true, deliverAs: "followUp" }).triggerTurn, true);
  // a resume when not tripped changes nothing
  assert.equal(clearTrip(f.host, f.r as unknown as Run), false);
  // the trip survives a restart through the last persisted entry
  assert.equal(trippedIn([entry(true)]), true);
  assert.equal(trippedIn([entry(true), entry(false)]), false);
  assert.equal(trippedIn([]), false);
  // the resumed window is empty: the next submission is at the bound, not over it
  assert.equal(loopGate(f.r, 5_000), false);
});

test("the loop guard counts only the submissions the host makes", () => {
  const f = fake({ bound: 2 });
  const now = 500_000;
  // nothing is counted until the host makes a submission through the gate
  assert.deepEqual(f.r.turnStarts, []);
  assert.equal(loopGate(f.r, now), false);
  assert.deepEqual(f.r.turnStarts, [now]);
  // each submission the host makes adds exactly one start; three over a bound of 2 trip
  assert.equal(loopGate(f.r, now + 1), false);
  assert.equal(loopGate(f.r, now + 2), true);
  assert.deepEqual(f.r.turnStarts, [now, now + 1, now + 2]);
});
