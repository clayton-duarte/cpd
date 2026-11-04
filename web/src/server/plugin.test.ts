import { describe, expect, it, vi } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import { cpdWebPlugin, type PluginBoundaries } from "./plugin.ts";

type Handler = (data: unknown, client: unknown) => void;

const flush = async (): Promise<void> => {
  for (let i = 0; i < 50; i += 1) await Promise.resolve();
};

const fakeClient = () => {
  const closers: Array<() => void> = [];
  return {
    send: vi.fn(),
    socket: { on: vi.fn((event: string, cb: () => void) => { if (event === "close") closers.push(cb); }) },
    closeSocket: () => closers.forEach((c) => c()),
  };
};

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

const fakeServer = () => {
  const handlers = new Map<string, Handler>();
  const httpHandlers = new Map<string, () => void>();
  const logger = { error: vi.fn() };
  const server = {
    ws: { on: vi.fn((event: string, cb: Handler) => { handlers.set(event, cb); }) },
    httpServer: { once: vi.fn((event: string, cb: () => void) => { httpHandlers.set(event, cb); }) },
    config: { logger },
  };
  return { server, handlers, httpHandlers, logger };
};

const configure = async (plugin: ReturnType<typeof cpdWebPlugin>, server: unknown): Promise<void> => {
  const hook = plugin.configureServer as unknown as (s: unknown) => unknown;
  await hook(server);
};

describe("cpd web plugin", () => {
  it("plugin registers the c2s handler on the vite ws and sends s2c to the client", async () => {
    const { b } = boundaries();
    const plugin = cpdWebPlugin({ boundaries: b });
    expect(plugin.name).toBe("cpd-web");
    expect(plugin.apply).toBe("serve");

    const { server, handlers } = fakeServer();
    await configure(plugin, server);
    const onC2s = handlers.get("cpd:c2s");
    expect(onC2s).toBeTypeOf("function");

    const a = fakeClient();
    const other = fakeClient();
    onC2s?.({ type: "nope" }, a);
    await flush();
    expect(a.send).toHaveBeenCalledTimes(1);
    expect(a.send.mock.calls[0]?.[0]).toBe("cpd:s2c");
    expect(a.send.mock.calls[0]?.[1]).toMatchObject({ type: "session.error", sessionId: null, error: { code: "invalid_message" } });
    expect(other.send).not.toHaveBeenCalled();
    expect(a.socket.on).toHaveBeenCalledWith("close", expect.any(Function));

    onC2s?.({ type: "nope" }, a);
    await flush();
    expect(a.send).toHaveBeenCalledTimes(2);
    expect(a.socket.on).toHaveBeenCalledTimes(1); // one connection per client, reused
  });

  it("plugin starts services on server start and closes them on server close", async () => {
    const { b, cpdRuns, followStarts } = boundaries();
    const plugin = cpdWebPlugin({ boundaries: b });
    const { server, handlers, httpHandlers, logger } = fakeServer();

    await configure(plugin, server);
    expect(cpdRuns.length).toBeGreaterThan(0); // cockpit.start() ran its initial load
    expect(cpdRuns.some((a) => a[0] === "context" && a[1] === "list")).toBe(true);
    expect(logger.error).toHaveBeenCalledTimes(1); // the failed start is logged, not thrown
    expect(followStarts).toHaveLength(0);

    const client = fakeClient();
    handlers.get("cpd:c2s")?.({ type: "cockpit.subscribe" }, client);
    await flush();
    expect(client.send.mock.calls[0]?.[1]).toMatchObject({ type: "cockpit.error" });
    const runsAfterStart = cpdRuns.length;

    expect(httpHandlers.has("close")).toBe(true);
    httpHandlers.get("close")?.();
    httpHandlers.get("close")?.(); // idempotent
    const sends = client.send.mock.calls.length;
    handlers.get("cpd:c2s")?.({ type: "cockpit.subscribe" }, client);
    await flush();
    expect(client.send.mock.calls.length).toBe(sends); // closed connections answer nothing
    expect(cpdRuns.length).toBe(runsAfterStart);
    expect(() => client.closeSocket()).not.toThrow();
  });
});
