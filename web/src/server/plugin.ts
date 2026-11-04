import { execFile, spawn } from "node:child_process";
import { homedir } from "node:os";
import { connect } from "node:net";
import { promises as fsp, watch as fsWatch } from "node:fs";
import type { Plugin, ViteDevServer, WebSocketClient } from "vite";
import type { Runner } from "../protocol/domain.ts";
import type { GitBoundary, Scheduler } from "../protocol/runtime.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { createCpd, type CpdExec, type FollowBoundary } from "./cockpit/cli.ts";
import { createCockpitService, type CockpitService } from "./cockpit/events.ts";
import { createReadModel } from "./cockpit/readmodel.ts";
import { connectHost, type SocketFactory } from "./durable/client.ts";
import { createDurableBackend } from "./durable/backend.ts";
import { createHubConnection, type HubConnection, type SessionBackend } from "./hub/connection.ts";
import { createRegistry, type Registry } from "./hub/registry.ts";
import { createPiBackend } from "./pi/backend.ts";
import type { FileBoundary } from "./pi/window.ts";
import type { WatchBoundary } from "./pi/watch.ts";

export interface PluginBoundaries {
  cpdExec: CpdExec;
  follow: FollowBoundary;
  socketFactory: SocketFactory;
  fs: FileBoundary;
  watch: WatchBoundary;
  git: GitBoundary;
  scheduler: Scheduler;
  cpdHome: string;
}
export interface Services {
  cockpit: CockpitService;
  registry: Registry;
  backends: Record<Runner, SessionBackend>;
  newConnection(send: (m: ServerMessage) => void): HubConnection;
}

/** Pure wiring of the modules; nothing is started. */
export function createServices(b: PluginBoundaries): Services {
  const cpd = createCpd(b.cpdExec);
  const cockpit = createCockpitService({ readModel: createReadModel(cpd), cpd, follow: b.follow, scheduler: b.scheduler });
  const registry = createRegistry({ snapshot: () => cockpit.snapshot(), cpdHome: b.cpdHome });
  // lazy: a backend is built on first attach so a host without sockets or files starts cleanly
  const lazy = (build: () => SessionBackend): SessionBackend => {
    let backend: SessionBackend | null = null;
    return { attach: (session, emit) => (backend ??= build()).attach(session, emit) };
  };
  const backends: Record<Runner, SessionBackend> = {
    pi: lazy(() => createPiBackend({ cpd, fs: b.fs, watch: b.watch, git: b.git })),
    durable: lazy(() =>
      createDurableBackend({
        connect: (socketPath) => connectHost(socketPath, { factory: b.socketFactory, scheduler: b.scheduler }),
        cpd,
        git: b.git,
        scheduler: b.scheduler,
      }),
    ),
  };
  return { cockpit, registry, backends, newConnection: (send) => createHubConnection({ cockpit, registry, backends, send }) };
}

// ---- real boundaries: untested glue, the only place that touches the outside world ----
const disposable = (f: () => void) => {
  let done = false;
  return { dispose: () => { if (!done) { done = true; f(); } } };
};

export function realBoundaries(env: NodeJS.ProcessEnv): PluginBoundaries {
  const cpdBin = env["CPD_BIN"] ?? "cpd";
  return {
    cpdExec: {
      run: (args) =>
        new Promise((resolve) => {
          execFile(cpdBin, [...args], { maxBuffer: 64 * 1024 * 1024 }, (error, stdout, stderr) => {
            const code = error === null ? 0 : typeof error.code === "number" ? error.code : 1;
            resolve({ stdout, stderr: error !== null && typeof error.code !== "number" ? error.message : stderr, exitCode: code });
          });
        }),
    },
    follow: {
      start: (args) => {
        const child = spawn(cpdBin, [...args], { stdio: ["ignore", "pipe", "pipe"] });
        let stderr = "";
        let pending = "";
        const lineHandlers = new Set<(line: string) => void>();
        const exitHandlers = new Set<(info: { code: number | null; stderr: string }) => void>();
        child.stderr.on("data", (c: Buffer) => { stderr += c.toString("utf8"); });
        child.stdout.on("data", (c: Buffer) => {
          pending += c.toString("utf8");
          let nl = pending.indexOf("\n");
          while (nl >= 0) {
            const line = pending.slice(0, nl);
            pending = pending.slice(nl + 1);
            for (const h of lineHandlers) h(line);
            nl = pending.indexOf("\n");
          }
        });
        child.on("close", (code) => { for (const h of exitHandlers) h({ code, stderr }); });
        return {
          onLine: (h) => { lineHandlers.add(h); return disposable(() => lineHandlers.delete(h)); },
          onExit: (h) => { exitHandlers.add(h); return disposable(() => exitHandlers.delete(h)); },
          kill: () => { child.kill(); },
        };
      },
    },
    socketFactory: {
      connect: (path) =>
        new Promise((resolve, reject) => {
          const socket = connect(path);
          socket.setEncoding("utf8");
          socket.once("error", reject);
          socket.once("connect", () => {
            socket.off("error", reject);
            resolve({
              write: (line) => { socket.write(line); },
              end: () => { socket.end(); },
              onData: (h) => { socket.on("data", h); return disposable(() => socket.off("data", h)); },
              onClose: (h) => {
                let error: Error | null = null;
                const onError = (e: Error) => { error = e; };
                const onClose = () => h(error);
                socket.on("error", onError);
                socket.on("close", onClose);
                return disposable(() => { socket.off("error", onError); socket.off("close", onClose); });
              },
            });
          });
        }),
    },
    fs: {
      stat: async (path) => {
        const s = await fsp.stat(path);
        return { size: s.size, identity: `${s.dev}:${s.ino}` };
      },
      read: async (path, start, end) => {
        const file = await fsp.open(path, "r");
        try {
          const buffer = new Uint8Array(Math.max(0, end - start));
          const { bytesRead } = await file.read(buffer, 0, buffer.length, start);
          return buffer.subarray(0, bytesRead);
        } finally {
          await file.close();
        }
      },
    },
    watch: {
      watch: (path, onChange) => {
        const watcher = fsWatch(path, () => onChange());
        return disposable(() => watcher.close());
      },
    },
    git: {
      branch: (cwd) =>
        new Promise((resolve) => {
          execFile("git", ["-C", cwd, "rev-parse", "--abbrev-ref", "HEAD"], (error, stdout) => resolve(error === null ? stdout.trim() || null : null));
        }),
    },
    scheduler: { now: () => Date.now(), after: (ms, run) => { const t = setTimeout(run, ms); return disposable(() => clearTimeout(t)); } },
    cpdHome: env["CPD_HOME"] ?? `${homedir()}/.cpd`,
  };
}

export function cpdWebPlugin(options?: { boundaries?: PluginBoundaries }): Plugin {
  let closeAll: (() => Promise<void>) | null = null;
  return {
    name: "cpd-web",
    apply: "serve",
    async configureServer(server: ViteDevServer) {
      const services = createServices(options?.boundaries ?? realBoundaries(process.env));
      const connections = new Map<WebSocketClient, HubConnection>();
      let closed = false;
      const started = services.cockpit.start().catch((e: unknown) => {
        server.config.logger.error(`cpd-web: cockpit failed to start: ${e instanceof Error ? e.message : String(e)}`);
      });
      closeAll = async () => {
        if (closed) return;
        closed = true;
        services.cockpit.stop();
        const all = [...connections.values()];
        connections.clear();
        await Promise.all(all.map((c) => c.close()));
      };
      server.ws.on("cpd:c2s", (data: unknown, client: WebSocketClient) => {
        if (closed) return;
        let hub = connections.get(client);
        if (hub === undefined) {
          const created = services.newConnection((m) => client.send("cpd:s2c", m));
          hub = created;
          connections.set(client, created);
          client.socket.on("close", () => {
            connections.delete(client);
            void created.close();
          });
        }
        const target = hub;
        void started
          .then(() => target.handle(data))
          .catch((e: unknown) => server.config.logger.error(`cpd-web: ${e instanceof Error ? e.message : String(e)}`));
      });
      server.httpServer?.once("close", () => { void closeAll?.(); });
      await started;
    },
    closeBundle() {
      return closeAll?.();
    },
  };
}
