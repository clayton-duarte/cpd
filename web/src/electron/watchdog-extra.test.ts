import { expect, it } from "vitest";
import type { HealthRecord } from "./health.ts";
import { createWatchdog } from "./watchdog.ts";

const T0 = Date.parse("2026-01-01T00:00:00.000Z");
const iso = (ms: number) => new Date(ms).toISOString();
const rec = (status: "healthy" | "crashed", epoch: number, ms: number): HealthRecord =>
  status === "healthy"
    ? { status, reloadEpoch: epoch, pid: 7, startedAt: iso(ms), healthyAt: iso(ms) }
    : { status, reloadEpoch: epoch, pid: 7, startedAt: iso(ms), crashedAt: iso(ms), source: "error-boundary", error: "x" };

function setup() {
  const calls: string[] = [];
  const logs: Array<Record<string, unknown>> = [];
  const s = { clock: T0, health: null as HealthRecord | null, alive: true };
  const wd = createWatchdog(
    {
      now: () => s.clock,
      readHealth: async () => s.health,
      pidAlive: () => s.alive,
      signalReload: (p) => void calls.push(`signal:${p}`),
      terminate: (p) => void calls.push(`terminate:${p}`),
      relaunch: async () => void calls.push("relaunch"),
      restore: {
        captureCandidate: async () => void calls.push("capture"),
        promoteCandidate: async () => void calls.push("promote"),
        hasLastGood: async () => true,
        restoreLastGood: async () => void calls.push("restore"),
        discardCandidate: async () => void calls.push("discard"),
      },
      log: async (e) => void logs.push(e as Record<string, unknown>),
    },
    { stabilizeMs: 3000, crashDebounceMs: 1000, loadingGraceMs: 10000 },
  );
  return { wd, s, calls, logs };
}

it("a missing health file does nothing", async () => {
  const t = setup();
  await t.wd.tick();
  expect(t.calls).toEqual([]);
  expect(t.logs).toEqual([]);
});

it("a promoted candidate re-arms the rollback", async () => {
  const t = setup();
  t.s.health = rec("crashed", 1, T0);
  await t.wd.tick();
  t.s.clock += 1000;
  await t.wd.tick();
  t.s.health = rec("healthy", 2, t.s.clock);
  await t.wd.tick();
  t.s.clock += 3000;
  await t.wd.tick();
  expect(t.calls).toContain("promote");
  t.s.health = rec("crashed", 3, t.s.clock);
  await t.wd.tick();
  t.s.clock += 1000;
  await t.wd.tick();
  expect(t.calls.filter((c) => c === "restore")).toHaveLength(2);
});
