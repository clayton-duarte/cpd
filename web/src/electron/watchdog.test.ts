import { expect, it } from "vitest";
import type { CockpitHealth, HealthRecord } from "./health.ts";
import { HealthParseError } from "./health.ts";
import { createHealthWatchdog, createPreviewWatchdog, createWatchdog, productionEnv } from "./watchdog.ts";
import type { Build, WatchdogEvent } from "./watchdog.ts";

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
    terminate: (pid: number) => {
      calls.push(`terminate:${pid}`);
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

it("rollback restores the last good build, terminates the live preview and relaunches in production", async () => {
  const s = setup();
  s.state.health = crashed(2);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture"]);
  s.advance(1000);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "restore", "terminate:100", "relaunch"]);
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
  expect(s.calls).not.toContain("terminate:100");
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
  expect(s.calls).toEqual(["capture", "restore", "terminate:100", "relaunch"]);
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

it("rollback relaunches without terminating when the preview pid is dead", async () => {
  const s = setup({ alive: false });
  s.state.health = healthy(2);
  await s.wd.tick();
  expect(s.calls).toEqual(["capture", "restore", "relaunch"]);
  expect(s.events()).toEqual(["rollback", "relaunch"]);
  expect(s.logs[1]).toMatchObject({ event: "relaunch", mode: "production" });
  expect(s.calls).not.toContain("terminate:100");
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
  expect(s.calls.filter((c) => c === "terminate:100")).toHaveLength(1);
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
  expect(s.calls).not.toContain("terminate:100");
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

it("rolls the preview back to production webui on loading too long", async () => {
  const calls: string[] = [];
  const logs: Entry[] = [];
  const state = { clock: T0, health: loading(2) as HealthRecord | null };
  const wd = createPreviewWatchdog(
    {
      now: () => state.clock,
      readHealth: async () => state.health,
      pidAlive: () => true,
      setMode: async (mode) => {
        calls.push(`mode:${mode}`);
      },
      showErrorBox: (title, content) => {
        calls.push(`dialog:${title}:${content}`);
      },
      log: async (entry) => {
        logs.push(entry as Entry);
      },
    },
    { stabilizeMs: 3000, crashDebounceMs: 1000, loadingGraceMs: 10000 },
  );
  await wd.tick();
  state.clock += 9999;
  await wd.tick();
  expect(calls).toEqual([]);
  state.clock += 1;
  await wd.tick();
  expect(calls).toEqual(["mode:webui", "dialog:WebUI Preview: loading too long"]);
  expect(logs.map((entry) => entry.event)).toContain("rollback");
  expect(calls).not.toContain("mode:webui-preview");
});

it("productionEnv sets CPD_MODE to production and drops the dev URL", () => {
  const env = productionEnv({ CPD_MODE: "preview", CPD_ELECTRON_DEV_URL: "http://localhost:5173/electron.html", HOME: "/h" });
  expect(env.CPD_MODE).toBe("production");
  expect("CPD_ELECTRON_DEV_URL" in env).toBe(false);
});

it("productionEnv keeps every other variable", () => {
  const input = { CPD_CHECKOUT: "/wt", HOME: "/h", PATH: "/bin" };
  expect(productionEnv({ ...input, CPD_ELECTRON_DEV_URL: "u" })).toEqual({ ...input, CPD_MODE: "production" });
});

function healthSetup(initial: { build?: Build } = {}) {
  const calls: string[] = [];
  const events: WatchdogEvent[] = [];
  const logs: Entry[] = [];
  const state = {
    clock: T0,
    health: null as CockpitHealth | null,
    build: (initial.build ?? "preview") as Build,
  };
  const wd = createHealthWatchdog({
    now: () => state.clock,
    readHealth: async () => state.health,
    build: () => state.build,
    revert: async (build: Build) => {
      calls.push(`revert:${build}`);
      state.build = build;
    },
    reloadRenderer: async () => {
      calls.push("reload-renderer");
    },
    report: async (event: WatchdogEvent) => {
      events.push(event);
    },
    log: async (entry: object) => {
      logs.push(entry as Entry);
    },
  });
  const advance = (ms: number) => {
    state.clock += ms;
  };
  return { wd, state, calls, events, logs, advance };
}

const noData = (epoch: number, startedAt: number): CockpitHealth => ({ epoch, startedAt, tree: null });

it("the ladder takes preview then prod then HEAD-1 then HEAD-2", async () => {
  const s = healthSetup({ build: "preview" });
  s.state.health = noData(1, T0);
  s.advance(10000);
  await s.wd.tick();
  expect(s.calls).toEqual(["revert:prod"]);
  s.state.health = noData(2, s.state.clock);
  s.advance(10000);
  await s.wd.tick();
  expect(s.calls).toEqual(["revert:prod", "revert:head-1"]);
  s.state.health = noData(3, s.state.clock);
  s.advance(10000);
  await s.wd.tick();
  expect(s.calls).toEqual(["revert:prod", "revert:head-1", "revert:head-2"]);
  // every rung has failed: no fourth step, and never a rung already tried
  s.state.health = noData(4, s.state.clock);
  s.advance(10000);
  await s.wd.tick();
  expect(s.calls).toEqual(["revert:prod", "revert:head-1", "revert:head-2"]);
});

it("each detection and recovery emits a structured event", async () => {
  const s = healthSetup({ build: "preview" });
  const health = noData(1, T0);
  s.state.health = health;
  s.advance(10000);
  await s.wd.tick();
  expect(s.events).toHaveLength(2);
  const detection = s.events[0];
  expect(detection).toMatchObject({ kind: "watchdog.break" });
  expect(detection?.data).toMatchObject({ detection: "empty-data", health });
  expect(typeof detection?.text).toBe("string");
  expect(detection?.text.length).toBeGreaterThan(0);
  const recovery = s.events[1];
  expect(recovery).toMatchObject({ kind: "watchdog.recovery" });
  expect(recovery?.data).toMatchObject({ detection: "empty-data", build: "prod" });
  expect(typeof recovery?.text).toBe("string");
  expect(recovery?.text.length).toBeGreaterThan(0);
  // one detection and one recovery per renderer boot
  s.advance(5000);
  await s.wd.tick();
  expect(s.events).toHaveLength(2);
});

it("cockpit not ready with no later snapshot reloads the renderer only", async () => {
  // The hub's subscribe runs before the cockpit's first load: it finds no snapshot, sends
  // cockpit.error "not ready" and drops the client, so no later snapshot ever comes
  // (web/src/server/hub/connection.ts:69-80; the root fix is Plan incident's cockpit-impl).
  const s = healthSetup({ build: "prod" });
  const health: CockpitHealth = { epoch: 1, startedAt: T0, tree: null, error: "not ready", errorAt: T0 };
  s.state.health = health;
  s.advance(9999);
  await s.wd.tick();
  expect(s.calls).toEqual([]);
  s.advance(1);
  await s.wd.tick();
  expect(s.calls).toEqual(["reload-renderer"]);
  expect(s.state.build).toBe("prod"); // the view stays: no ladder step, no app restart
  const detection = s.events[0];
  expect(detection).toMatchObject({ kind: "watchdog.break" });
  expect(detection?.data).toMatchObject({ detection: "cockpit-not-ready", health });
  const recovery = s.events[1];
  expect(recovery).toMatchObject({ kind: "watchdog.recovery" });
  expect(recovery?.data).toMatchObject({ detection: "cockpit-not-ready", recovery: "renderer-reload" });
  // no second reload, and still no ladder step, while the break persists in this boot
  s.advance(60000);
  await s.wd.tick();
  expect(s.calls).toEqual(["reload-renderer"]);
  expect(s.state.build).toBe("prod");
});
