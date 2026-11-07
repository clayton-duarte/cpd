import { describe, expect, it, vi } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import { cpdWebPlugin, type PluginBoundaries } from "./plugin.ts";

// Every cpd call fails: the cockpit never becomes ready, which is observable without cpd JSON fixtures.
const boundaries = () => {
  const cpdRuns: Array<readonly string[]> = [];
  const followStarts: Array<readonly string[]> = [];
  const b = {
    cpdExec: { run: (args: readonly string[]) => { cpdRuns.push(args); return Promise.resolve({ stdout: "", stderr: "cpd down", exitCode: 1 }); } },
    follow: { start: (args: readonly string[]) => { followStarts.push(args); throw new Error("follow must not start after a failed load"); } },
    socketFactory: () => { throw new Error("no sockets in tests"); },
    fs: {},
    watch: {},
    git: { branch: () => Promise.resolve(null) },
    scheduler: new FakeScheduler(),
    cpdHome: "/fake/cpd-home",
  } as unknown as PluginBoundaries;
  return { b, cpdRuns, followStarts };
};

type UpgradeListener = (request: unknown, socket: unknown, head: unknown) => void;

const fakeServer = () => {
  const upgrades: UpgradeListener[] = [];
  const httpHandlers = new Map<string, () => void>();
  const logger = { error: vi.fn() };
  const server = {
    httpServer: {
      on: vi.fn((event: string, cb: UpgradeListener) => { if (event === "upgrade") upgrades.push(cb); }),
      off: vi.fn(),
      once: vi.fn((event: string, cb: () => void) => { httpHandlers.set(event, cb); }),
    },
    config: { logger },
  };
  return { server, upgrades, httpHandlers, logger };
};

const configure = async (plugin: ReturnType<typeof cpdWebPlugin>, server: unknown): Promise<void> => {
  const hook = plugin.configureServer as unknown as (s: unknown) => unknown;
  await hook(server);
};

describe("cpd web plugin", () => {
  it("hosts the one WebSocket endpoint on the dev http server", async () => {
    const { b, cpdRuns, followStarts } = boundaries();
    const plugin = cpdWebPlugin({ boundaries: b });
    expect(plugin.name).toBe("cpd-web");
    expect(plugin.apply).toBe("serve");

    const { server, upgrades, logger } = fakeServer();
    await configure(plugin, server);
    expect(server.httpServer.on).toHaveBeenCalledWith("upgrade", expect.any(Function));
    expect(upgrades).toHaveLength(1);
    expect(cpdRuns.length).toBeGreaterThan(0); // cockpit.start() ran its initial load
    expect(cpdRuns.some((a) => a[0] === "context" && a[1] === "list")).toBe(true);
    expect(logger.error).toHaveBeenCalledTimes(1); // the failed start is logged, not thrown
    expect(followStarts).toHaveLength(0);
  });

  it("closes services and the endpoint on http server close, idempotently", async () => {
    const { b, cpdRuns } = boundaries();
    const plugin = cpdWebPlugin({ boundaries: b });
    const { server, httpHandlers } = fakeServer();

    await configure(plugin, server);
    const runsAfterStart = cpdRuns.length;

    expect(httpHandlers.has("close")).toBe(true);
    httpHandlers.get("close")?.();
    httpHandlers.get("close")?.(); // idempotent
    expect(cpdRuns.length).toBe(runsAfterStart);
  });
});
