import { describe, expect, it, vi } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServices, cpdWebPlugin, type PluginBoundaries } from "./plugin.ts";

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
      prependListener: vi.fn((_event: string, _handler: unknown) => {}),
      once: vi.fn((event: string, cb: () => void) => { httpHandlers.set(event, cb); }),
    },
    config: { logger, server: { host: "127.0.0.1" } },
    middlewares: { use: vi.fn((_handler: unknown) => {}) },
  };
  return { server, upgrades, httpHandlers, logger };
};

const configure = async (plugin: ReturnType<typeof cpdWebPlugin>, server: unknown): Promise<void> => {
  const hook = plugin.configureServer as unknown as (s: unknown) => unknown;
  await hook(server);
};

describe("cpd web plugin", () => {
  it("refuses unauthenticated LAN preview before starting cockpit work", async () => {
    const { b, cpdRuns } = boundaries();
    const { server } = fakeServer();
    server.config.server.host = "0.0.0.0";
    await expect(configure(cpdWebPlugin({ boundaries: b }), server)).rejects.toThrow("requires CPD_WEBUI_ACCESS_TOKEN");
    expect(cpdRuns).toEqual([]);
  });

  it("authenticates preview HTTP and HMR upgrades when LAN access is enabled", async () => {
    const token = "preview-backend-token-".repeat(3);
    vi.stubEnv("CPD_WEBUI_ACCESS_TOKEN", token);
    try {
      const { server } = fakeServer();
      server.config.server.host = "0.0.0.0";
      await configure(cpdWebPlugin({ boundaries: boundaries().b }), server);
      const middleware = server.middlewares.use.mock.calls[0]![0] as (req: unknown, res: unknown, next: () => void) => void;
      const response = { writeHead: vi.fn(), end: vi.fn() };
      const next = vi.fn();
      middleware({ headers: {} }, response, next);
      expect(response.writeHead.mock.calls[0]![0]).toBe(401);
      expect(next).not.toHaveBeenCalled();
      middleware({ headers: { authorization: `Basic ${Buffer.from(`cpd:${token}`).toString("base64")}` } }, response, next);
      expect(next).toHaveBeenCalledOnce();
      const upgrade = server.httpServer.prependListener.mock.calls[0]![1] as (req: unknown, socket: unknown) => void;
      const socket = { write: vi.fn(), destroy: vi.fn() };
      upgrade({ headers: {} }, socket);
      expect(socket.destroy).toHaveBeenCalledOnce();
    } finally { vi.unstubAllEnvs(); }
  });
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


it("production services share persisted action results across connections and restarts", async () => {
  const dir = mkdtempSync(join(tmpdir(), "cpd-dedup-"));
  try {
    const { b } = boundaries();
    b.cpdHome = dir;
    const services = createServices(b);
    const run = vi.fn(async () => ({ stdout: "created" }));
    services.cockpit.runAction = run;
    const action = { type: "cockpit.action", actionId: "reload-safe", action: { kind: "inbox.dismiss", itemId: 4 } };
    await services.newConnection(() => {}).handle(action);
    await services.newConnection(() => {}).handle(action);
    expect(run).toHaveBeenCalledTimes(1);
    const restarted = createServices(b);
    const rerun = vi.fn(async () => ({ stdout: "duplicate" }));
    restarted.cockpit.runAction = rerun;
    const sent: unknown[] = [];
    await restarted.newConnection((m) => sent.push(m)).handle(action);
    expect(rerun).not.toHaveBeenCalled();
    expect(sent).toContainEqual({ type: "action.result", actionId: "reload-safe", ok: true, output: { stdout: "created" } });
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
