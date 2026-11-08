// The health watchdog's ladder step, in order (air-d127): the rung is applied, then the recovery
// Event is committed, and only then does the app restart. Reporting after the restart loses the
// Event to the process exiting mid-write; reporting before the rung is applied would lie whenever
// the copy fails. The e2e ladder walk (web/src/testing/e2e-watchdog.spec.ts) proves the walk by the
// loaded view; this test pins the order the walk depends on.
import { describe, expect, it } from "vitest";
import type { CockpitHealth } from "./health.ts";
import { createHealthWatchdog, type HealthWatchdogPorts, type WatchdogEvent } from "./watchdog.ts";

const T0 = 1_700_000_000_000;

type Recorded = { calls: string[]; events: WatchdogEvent[] };

/** Ports that record, in one list, every step the watchdog takes in the order it takes them. */
function portsRecord(state: { build: string }, clock: { now: number }): { ports: HealthWatchdogPorts; recorded: Recorded } {
  const recorded: Recorded = { calls: [], events: [] };
  const ports: HealthWatchdogPorts = {
    now: () => clock.now,
    readHealth: async () => null,
    build: () => state.build as never,
    revert: async (build) => {
      recorded.calls.push(`apply:${build}`);
      state.build = build;
    },
    relaunch: async (mode) => {
      recorded.calls.push(`relaunch:${mode}`);
    },
    reloadRenderer: async () => {
      recorded.calls.push("reload-renderer");
    },
    report: async (event) => {
      recorded.events.push(event);
      recorded.calls.push(`report:${event.kind}:${event.data["build"] ?? event.data["recovery"] ?? event.data["detection"]}`);
    },
    log: async () => {},
  };
  return { ports, recorded };
}

/** A broken report: no cockpit snapshot 20 s after boot (`empty-data`). */
const emptyData = (epoch: number, startedAt: number): CockpitHealth => ({ epoch, startedAt, tree: null });

/** A clock 20 s past boot, so the empty tree has been empty long enough to be a break. */
const LATE = { now: T0 + 20_000 };

describe("the ladder step's order (air-d127)", () => {
  it("applies the rung, reports the recovery Event, then relaunches in production", async () => {
    const state = { build: "preview" };
    const { ports, recorded } = portsRecord(state, LATE);
    ports.readHealth = async () => emptyData(1, T0);
    const watchdog = createHealthWatchdog(ports, { snapshotGraceMs: 10_000, heartbeatTimeoutMs: 5_000 });

    await watchdog.tick();

    expect(recorded.calls).toEqual(["report:watchdog.break:empty-data", "apply:prod", "report:watchdog.recovery:prod", "relaunch:production"]);
    expect(recorded.events[1]).toMatchObject({ kind: "watchdog.recovery", data: { detection: "empty-data", build: "prod" } });
    // The rung is the one in force when the Event is committed, and the restart asks for production.
    expect(state.build).toBe("prod");
  });

  it("reports nothing and does not relaunch when the rung cannot be applied", async () => {
    const state = { build: "prod" };
    const { ports, recorded } = portsRecord(state, LATE);
    ports.readHealth = async () => emptyData(1, T0);
    ports.revert = async () => {
      recorded.calls.push("apply:head-1");
      throw new Error("copy failed");
    };
    const watchdog = createHealthWatchdog(ports, { snapshotGraceMs: 10_000, heartbeatTimeoutMs: 5_000 });

    await expect(watchdog.tick()).rejects.toThrow("copy failed");

    // A recovery that did not happen is never announced, and the app is not restarted onto the build
    // that is still in place.
    expect(recorded.calls).toEqual(["report:watchdog.break:empty-data", "apply:head-1"]);
  });

  it("does not relaunch when every rung has failed", async () => {
    const state = { build: "head-2" };
    const { ports, recorded } = portsRecord(state, LATE);
    ports.readHealth = async () => emptyData(1, T0);
    const watchdog = createHealthWatchdog(ports, { snapshotGraceMs: 10_000, heartbeatTimeoutMs: 5_000 });

    await watchdog.tick();

    expect(recorded.calls).toEqual(["report:watchdog.break:empty-data"]);
  });
});
