import { expect, it } from "vitest";
import type { HealthRecord } from "./health.ts";
import { HealthParseError } from "./health.ts";
import { createWatchdog } from "./watchdog.ts";

const T0 = Date.parse("2026-01-01T00:00:00.000Z");
const iso = (ms: number) => new Date(ms).toISOString();

function loading(epoch: number, startedMs = T0, pid = 100): HealthRecord {
  return { status: "loading", reloadEpoch: epoch, pid, startedAt: iso(startedMs) };
}
function healthy(epoch: number, startedMs = T0, pid = 100): HealthRecord {
  return { status: "healthy", reloadEpoch: epoch, pid, startedAt: iso(startedMs), healthyAt: iso(startedMs) };
}
function crashed(epoch: number, startedMs = T0, pid = 100): HealthRecord {
  return {
    status: "crashed",
    reloadEpoch: epoch,
    pid,
    startedAt: iso(startedMs),
    crashedAt: iso(startedMs),
    source: "error-boundary",
    error: "boom",
  };
}
function exited(epoch: number, pid = 100): HealthRecord {
  return { status: "exited", reloadEpoch: epoch, pid, startedAt: iso(T0), exitedAt: iso(T0) };
}

type Entry = Record<string, unknown>;

function setup(opts: { lastGood?: boolean; alive?: boolean; restoreError?: Error } = {}) {
  const calls: string[] = [];
  const logs: Entry[] = [];
  const state = {
    clock: T0,
    health: null as HealthRecord | null | Error,
    alive: opts.alive ?? true,
    lastGood: opts.lastGood ?? true,
  };
  const ports = {
    now: () => state.clock,
    readHealth: async () => {
      if (state.health instanceof Error) throw state.health;
      return state.health;
    },
    pidAlive: (_pid: number) => state.alive,
    signalReload: (pid: number) => {
      calls.push(`signal:${pid}`);
    },
    relaunch: async () => {
      calls.push("relaunch");
    },
    restore: {
      captureCandidate: async () => {
        calls.push("capture");
      },
      promoteCandidate: async () => {
        calls.push("promote");
      },
      hasLastGood: async () => state.lastGood,
      restoreLastGood: async () => {
        calls.push("restore");
        if (opts.restoreError) throw opts.restoreError;
      },
      discardCandidate: async () => {
        calls.push("discard");
      },
    },
    log: async (entry: object) => {
      logs.push(entry as Entry);
    },
  };
  const wd = createWatchdog(ports, { stabilizeMs: 3000, crashDebounceMs: 1000, loadingGraceMs: 10000 });
  const advance = (ms: number) => {
    state.clock += ms;
  };
  const events = () => logs.map((l) => l.event);
  return { wd, state, calls, logs, advance, events };
}

it("first observation of an epoch captures a candidate", async () => {
  const s = setup();
  s.state.health = loading(1);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
  s.state.health = loading(2, T0);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "capture"]);
});

it("healthy for the stabilization window promotes the candidate", async () => {
  const s = setup();
  s.state.health = healthy(1);
  await s.wd.tick();
  s.advance(2999);
  await s.wd.tick();
  expect(s.calls).not.toContain("promote");
  s.advance(1);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "promote"]);
  s.advance(5000);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "promote"]);
});

it("healthy for less than the window promotes nothing", async () => {
  const s = setup();
  s.state.health = healthy(1);
  await s.wd.tick();
  s.advance(2000);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
});

it("a candidate of a crashed epoch is never promoted", async () => {
  const s = setup({ lastGood: false });
  s.state.health = healthy(1);
  await s.wd.tick();
  s.advance(1000);
  s.state.health = crashed(1);
  await s.wd.tick();
  s.advance(10000);
  await s.wd.tick();
  expect(s.calls).not.toContain("promote");
});

it("a crash past the debounce rolls back, signals SIGUSR1 and logs", async () => {
  const s = setup();
  s.state.health = crashed(2);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
  s.advance(1000);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "restore", "signal:100"]);
  expect(s.logs[0]).toMatchObject({ event: "rollback", reloadEpoch: 2, pid: 100 });
  expect(typeof s.logs[0]?.reason).toBe("string");
});

it("a crash that clears within the debounce does nothing", async () => {
  const s = setup();
  s.state.health = crashed(2);
  await s.wd.tick();
  s.advance(500);
  s.state.health = loading(3, T0 + 500);
  await s.wd.tick();
  s.advance(1500);
  s.state.health = healthy(3, T0 + 500);
  await s.wd.tick();
  expect(s.calls).not.toContain("restore");
  expect(s.calls).not.toContain("signal:100");
  expect(s.logs).toEqual([]);
});

it("loading past the grace period rolls back", async () => {
  const s = setup();
  s.state.health = loading(2);
  await s.wd.tick();
  s.advance(9999);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
  s.advance(1);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "restore", "signal:100"]);
  expect(s.events()).toEqual(["rollback"]);
});

it("loading that turns healthy within the grace period does nothing", async () => {
  const s = setup();
  s.state.health = loading(2);
  await s.wd.tick();
  s.advance(5000);
  s.state.health = healthy(2);
  await s.wd.tick();
  s.advance(6000);
  await s.wd.tick();
  expect(s.calls).not.toContain("restore");
  expect(s.logs).toEqual([]);
});

it("a dead pid without an exited status rolls back and relaunches", async () => {
  const s = setup({ alive: false });
  s.state.health = healthy(2);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "restore", "relaunch"]);
  expect(s.events()).toEqual(["rollback", "relaunch"]);
  expect(s.calls).not.toContain("signal:100");
});

it("an exited status never rolls back", async () => {
  const s = setup({ alive: false });
  s.state.health = exited(2);
  await s.wd.tick();
  s.advance(60000);
  await s.wd.tick();
  expect(s.calls).not.toContain("restore");
  expect(s.calls).not.toContain("relaunch");
  expect(s.logs).toEqual([]);
});

it("rollback without a restore point only logs", async () => {
  const s = setup({ lastGood: false });
  s.state.health = crashed(2);
  await s.wd.tick();
  s.advance(1000);
  await s.wd.tick();
  s.advance(1000);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
  expect(s.events()).toEqual(["rollback-skipped"]);
});

it("a failed epoch after a rollback is not rolled back again", async () => {
  const s = setup();
  s.state.health = crashed(2);
  await s.wd.tick();
  s.advance(1000);
  await s.wd.tick();
  expect(s.events()).toEqual(["rollback"]);
  s.advance(100);
  s.state.health = crashed(3, s.state.clock);
  await s.wd.tick();
  s.advance(1000);
  await s.wd.tick();
  s.advance(1000);
  await s.wd.tick();
  expect(s.calls.filter((c) => c === "restore")).toHaveLength(1);
  expect(s.calls.filter((c) => c === "signal:100")).toHaveLength(1);
  expect(s.logs.filter((l) => l.event === "rollback-skipped")).toHaveLength(1);
  expect(s.logs.find((l) => l.event === "rollback-skipped")?.reason).toBe("already rolled back");
});

it("a restore failure is logged and the watchdog keeps ticking", async () => {
  const s = setup({ restoreError: new Error("disk full") });
  s.state.health = crashed(2);
  await s.wd.tick();
  s.advance(1000);
  await s.wd.tick();
  expect(s.events()).toContain("rollback-failed");
  expect(s.calls).not.toContain("signal:100");
  s.state.health = healthy(3, s.state.clock);
  s.advance(100);
  await expect(s.wd.tick()).resolves.toBeUndefined();
  expect(s.calls.filter((c) => c === "capture")).toHaveLength(2);
});

it("a malformed health file is logged once and ignored", async () => {
  const s = setup();
  s.state.health = new HealthParseError("bad json {");
  await s.wd.tick();
  await s.wd.tick();
  expect(s.logs).toHaveLength(1);
  expect(s.calls).toEqual([]);
  s.state.health = new HealthParseError("other garbage");
  await s.wd.tick();
  expect(s.logs).toHaveLength(2);
  expect(s.calls).toEqual([]);
});
