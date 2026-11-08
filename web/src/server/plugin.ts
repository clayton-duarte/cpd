import { execFile, spawn } from "node:child_process";
import { homedir, tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";
import { dirname, join } from "node:path";
import { connect } from "node:net";
import { existsSync, statSync, promises as fsp, realpathSync, watch as fsWatch } from "node:fs";
import type { Plugin, ViteDevServer } from "vite";
import type { Runner } from "../protocol/domain.ts";
import type { GitBoundary, Scheduler } from "../protocol/runtime.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { createCpd, type CpdExec, type FollowBoundary } from "./cockpit/cli.ts";
import { toArgv } from "./cpd-argv.ts";
import { createCockpitService, type CockpitService, type Extras } from "./cockpit/events.ts";
import { readCosts, type CostFile } from "./cockpit/cost.ts";
import { readLogs } from "./cockpit/logs.ts";
import { buildPlanView, createReadModel, parsePlanShow } from "./cockpit/readmodel.ts";
import { readSettings, saveSettings } from "./cockpit/settings.ts";
import { createCockpitStream, type StreamChild } from "./cockpit/stream.ts";
import { readDrift } from "../electron/drift.ts";
import type { UpdateExec } from "../electron/update/types.ts";
import { readStatus } from "./cockpit/status.ts";
import { connectHost, type SocketFactory } from "./durable/client.ts";
import { createDurableBackend } from "./durable/backend.ts";
import { createCompletionClassifier, type CompletionClassifier } from "./completion-classify.ts";
import { createCompletionService } from "./completion-service.ts";
import { createHubConnection, type HubConnection, type SessionBackend } from "./hub/connection.ts";
import { createUpgradeFactory, createWsServer, WS_PATH, type WebSocketUpgradeServer } from "./ws.ts";
import { createRegistry, type Registry } from "./hub/registry.ts";
import { createPiBackend } from "./pi/backend.ts";
import { createLiveSource, type LiveConnection } from "./pi/live.ts";
import type { FileBoundary } from "./pi/window.ts";
import type { WatchBoundary } from "./pi/watch.ts";

/** A unix socket as lines. Connection errors (ENOENT, ECONNREFUSED, resets) are swallowed into `onClose`; nothing throws into the server. */
function connectLive(path: string): LiveConnection {
  const socket = connect(path);
  socket.setEncoding("utf8");
  let rest = "";
  socket.on("error", () => {});
  return {
    onLine(cb) {
      socket.on("data", (chunk: string) => {
        rest += chunk;
        let nl = rest.indexOf("\n");
        while (nl >= 0) {
          const line = rest.slice(0, nl);
          rest = rest.slice(nl + 1);
          if (line !== "") cb(line);
          nl = rest.indexOf("\n");
        }
      });
    },
    onClose(cb) {
      socket.once("close", cb);
    },
    close() {
      socket.destroy();
    },
  };
}

export interface PluginBoundaries {
  cpdExec: CpdExec;
  follow: FollowBoundary;
  /** Spawns `cpd <args>` as the long-lived cockpit stream child; absent, the service uses the spawn refresh alone. */
  stream?: (args: string[]) => StreamChild;
  socketFactory: SocketFactory;
  fs: FileBoundary;
  watch: WatchBoundary;
  git: GitBoundary;
  scheduler: Scheduler;
  cpdHome: string;
  /** Classifies a Lead session's completion; absent, no badge is computed. */
  classify?: CompletionClassifier;
  /** Runs `tmux <args>`; rejects on a non-zero exit. */
  tmux(args: readonly string[]): Promise<string>;
  /** Reads a whole text file with its stat; null when it does not exist. */
  readText(path: string): Promise<CostFile | null>;
}
export interface Services {
  cockpit: CockpitService;
  registry: Registry;
  backends: Record<Runner, SessionBackend>;
  newConnection(send: (m: ServerMessage) => void): HubConnection;
}

const updateExec: UpdateExec = (cmd, args, opts) =>
  new Promise((resolve) => {
    execFile(cmd, args, { cwd: opts.cwd, env: opts.env, maxBuffer: 64 * 1024 * 1024 }, (error, stdout, stderr) => {
      const code = error === null ? 0 : typeof error.code === "number" ? error.code : 1;
      resolve({ code, stdout, stderr: stderr || (error?.message ?? "") });
    });
  });

/** Pure wiring of the modules; nothing is started. */
export function createServices(b: PluginBoundaries): Services {
  const cpd = createCpd(b.cpdExec);
  const cpdText = async (argv: readonly string[]): Promise<string> => {
    const r = await b.cpdExec.run(argv);
    if (r.exitCode !== 0) throw new Error(r.stderr.trim() || `cpd exited ${r.exitCode}`);
    return r.stdout;
  };
  const DURABLE_COST_SQL = `SELECT sum(
  (SELECT coalesce(sum(json_extract(v.value,'$.cost.total')),0) FROM json_each(r.content,'$.models') v)
+ (SELECT coalesce(sum(json_extract(v.value,'$.cost.total')),0) FROM json_each(r.content,'$.tools') v)
) AS cost
FROM document_revisions r JOIN documents d ON d.id = r.document_id
WHERE d.kind = '"pi.usage"' AND d.retired_at IS NULL AND r.kind = 'base'`;
  const statOrNull = async (path: string) => fsp.stat(path).catch((e: unknown) => { if ((e as { code?: unknown }).code === "ENOENT") return null; throw e; });
  const durableCost = async (id: string): Promise<{ cost: number | null; stamp: string } | null> => {
    const file = `${b.cpdHome}/run/host/${id}.sqlite`;
    const main = await statOrNull(file);
    if (main === null) return null;
    const wal = await statOrNull(`${file}-wal`);
    const stamp = `${main.mtimeMs}:${main.size}:${wal?.mtimeMs ?? 0}:${wal?.size ?? 0}`;
    // Built-in node:sqlite, not chord's binding: that one writes the host store (journal_mode, checkpoint on close).
    const db = new DatabaseSync(file, { readOnly: true });
    try {
      const row = db.prepare(DURABLE_COST_SQL).get() as { cost: number | null } | undefined;
      return { cost: row?.cost ?? null, stamp };
    } finally {
      db.close();
    }
  };
  const extras: Extras = {
    status: () => readStatus({ cpd: cpdText, tmux: b.tmux, readFile: async (path) => (await b.readText(path))?.text ?? null, env: process.env, drift: (root) => readDrift({ exec: updateExec, mtimeMs: (path) => { try { return statSync(path).mtimeMs; } catch { return null; } }, now: Date.now }, root) }),
    costs: () => readCosts({ cpd: cpdText, readFile: b.readText, durableCost }),
    logs: () => readLogs({ cpd: cpdText }),
    settings: () => readSettings(cpd),
    saveSettings: (changes) => saveSettings(cpd, changes),
  };
  const cockpit = createCockpitService({ readModel: createReadModel(cpd), cpd, follow: b.follow, scheduler: b.scheduler, extras, ...(b.stream !== undefined ? { stream: createCockpitStream({ spawn: b.stream, scheduler: b.scheduler }) } : {}) });
  const registry = createRegistry({ snapshot: () => cockpit.snapshot(), cpdHome: b.cpdHome });
  // lazy: a backend is built on first attach so a host without sockets or files starts cleanly
  const lazy = (build: () => SessionBackend): SessionBackend => {
    let backend: SessionBackend | null = null;
    return { attach: (session, emit) => (backend ??= build()).attach(session, emit) };
  };
  const backends: Record<Runner, SessionBackend> = {
    pi: lazy(() => createPiBackend({ cpd, fs: b.fs, watch: b.watch, git: b.git, cpdHome: b.cpdHome, attachmentsDir: join(tmpdir(), "cpd-webui-attachments"), now: Date.now, scheduler: b.scheduler, live: () => createLiveSource({ connect: connectLive, scheduler: b.scheduler, cpdHome: b.cpdHome }) })),
    durable: lazy(() =>
      createDurableBackend({
        connect: (socketPath) => connectHost(socketPath, { factory: b.socketFactory, scheduler: b.scheduler }),
        cpd,
        git: b.git,
        scheduler: b.scheduler,
      }),
    ),
  };
  const completion = b.classify === undefined ? undefined : createCompletionService({
    classifier: b.classify,
    planOf: async (ref) => {
      const node = (cockpit.snapshot().tree?.contexts ?? []).flatMap((c) => c.plans.flatMap((p) => p.sessions)).find((n) => n.ref.id === ref.id);
      const planId = node?.planId ?? null;
      if (planId === null) return null;
      return buildPlanView(parsePlanShow(await cpd.json(toArgv({ kind: "plan.show", planId }))));
    },
  });
  return { cockpit, registry, backends, newConnection: (send) => createHubConnection({ cockpit, registry, backends, send, ...(completion === undefined ? {} : { completion }) }) };
}

// ---- real boundaries: untested glue, the only place that touches the outside world ----
const disposable = (f: () => void) => {
  let done = false;
  return { dispose: () => { if (!done) { done = true; f(); } } };
};

export const CPD_IDENTITY_VARS: readonly string[] = ["CPD_SESSION", "PI_SESSION_ID", "CPD_SESSION_ID", "CPD_ATTEMPT_ID"];

/** The environment cpd children get: the server's own, minus any cpd/pi session identity. */
export function cpdChildEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const out = { ...env };
  for (const k of CPD_IDENTITY_VARS) delete out[k];
  return out;
}

export interface ChildBoundary {
  execFile(
    file: string,
    args: string[],
    options: { maxBuffer: number; env?: NodeJS.ProcessEnv },
    callback: (error: (Error & { code?: unknown }) | null, stdout: string, stderr: string) => void,
  ): unknown;
  spawn(
    file: string,
    args: string[],
    options: { stdio: ["ignore" | "pipe", "pipe", "pipe"]; env?: NodeJS.ProcessEnv },
  ): {
    stdin?: { write(chunk: string): unknown; end(): unknown; on(event: "error", h: () => void): unknown } | null;
    stdout: { on(event: "data", h: (c: Buffer) => void): unknown };
    stderr: { on(event: "data", h: (c: Buffer) => void): unknown };
    on(event: "close", h: (code: number | null) => void): unknown;
    kill(): unknown;
  };
}

/** `<dir of the cpd binary>/../share/cpd/pi/lib/classify.ts` (core/src/config/file.rs pi_adapter_dir), null when absent. */
function completionScript(env: NodeJS.ProcessEnv, cpdBin: string): string | null {
  try {
    const bin = cpdBin.includes("/") ? cpdBin : (env["PATH"] ?? "").split(":").filter((d) => d !== "").map((d) => join(d, cpdBin)).find((p) => existsSync(p));
    if (bin === undefined) return null;
    const script = join(dirname(realpathSync(bin)), "..", "share", "cpd", "pi", "lib", "classify.ts");
    return existsSync(script) ? script : null;
  } catch {
    return null;
  }
}

export function realBoundaries(env: NodeJS.ProcessEnv, child: ChildBoundary = { execFile, spawn: spawn as ChildBoundary["spawn"] }): PluginBoundaries {
  const cpdBin = env["CPD_BIN"] ?? "cpd";
  const scheduler: Scheduler = { now: () => Date.now(), after: (ms, run) => { const t = setTimeout(run, ms); return disposable(() => clearTimeout(t)); } };
  const script = completionScript(env, cpdBin);
  return {
    classify: createCompletionClassifier({ child, scheduler, script, agentDir: env["PI_CODING_AGENT_DIR"] ?? join(homedir(), ".pi", "agent"), env: cpdChildEnv(env) }),
    cpdExec: {
      run: (args) =>
        new Promise((resolve) => {
          child.execFile(cpdBin, [...args], { maxBuffer: 64 * 1024 * 1024, env: cpdChildEnv(env) }, (error, stdout, stderr) => {
            const code = error === null ? 0 : typeof error.code === "number" ? error.code : 1;
            resolve({ stdout, stderr: error !== null && typeof error.code !== "number" ? error.message : stderr, exitCode: code });
          });
        }),
    },
    follow: {
      start: (args) => {
        const proc = child.spawn(cpdBin, [...args], { stdio: ["ignore", "pipe", "pipe"], env: cpdChildEnv(env) });
        let stderr = "";
        let pending = "";
        const lineHandlers = new Set<(line: string) => void>();
        const exitHandlers = new Set<(info: { code: number | null; stderr: string }) => void>();
        proc.stderr.on("data", (c: Buffer) => { stderr += c.toString("utf8"); });
        proc.stdout.on("data", (c: Buffer) => {
          pending += c.toString("utf8");
          let nl = pending.indexOf("\n");
          while (nl >= 0) {
            const line = pending.slice(0, nl);
            pending = pending.slice(nl + 1);
            for (const h of lineHandlers) h(line);
            nl = pending.indexOf("\n");
          }
        });
        proc.on("close", (code) => { for (const h of exitHandlers) h({ code, stderr }); });
        return {
          onLine: (h) => { lineHandlers.add(h); return disposable(() => lineHandlers.delete(h)); },
          onExit: (h) => { exitHandlers.add(h); return disposable(() => exitHandlers.delete(h)); },
          kill: () => { proc.kill(); },
        };
      },
    },
    stream: (args) => {
      const proc = child.spawn(cpdBin, [...args], { stdio: ["pipe", "pipe", "pipe"], env: cpdChildEnv(env) });
      let stderr = "";
      let pending = "";
      let lineHandler: (line: string) => void = () => undefined;
      let exitHandler: (info: { code: number | null; stderr: string }) => void = () => undefined;
      // a write to a dead child emits EPIPE here; the close event reports the exit
      proc.stdin?.on("error", () => undefined);
      proc.stderr.on("data", (c: Buffer) => { stderr += c.toString("utf8"); });
      proc.stdout.on("data", (c: Buffer) => {
        pending += c.toString("utf8");
        let nl = pending.indexOf("\n");
        while (nl >= 0) {
          const line = pending.slice(0, nl);
          pending = pending.slice(nl + 1);
          lineHandler(line);
          nl = pending.indexOf("\n");
        }
      });
      proc.on("close", (code) => exitHandler({ code, stderr }));
      return {
        write: (line) => { proc.stdin?.write(line); },
        endInput: () => { proc.stdin?.end(); },
        kill: () => { proc.kill(); },
        onLine: (h) => { lineHandler = h; },
        onExit: (h) => { exitHandler = h; },
      };
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
      write: async (path, bytes) => {
        await fsp.mkdir(dirname(path), { recursive: true });
        await fsp.writeFile(path, bytes, { mode: 0o600 });
      },
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
    tmux: (args) =>
      new Promise((resolve, reject) => {
        child.execFile("tmux", [...args], { maxBuffer: 1024 * 1024, env: cpdChildEnv(env) }, (error, stdout) => (error === null ? resolve(stdout) : reject(error)));
      }),
    readText: async (path) => {
      try {
        const [text, s] = await Promise.all([fsp.readFile(path, "utf8"), fsp.stat(path)]);
        return { text, mtimeMs: s.mtimeMs, size: s.size };
      } catch (e) {
        if ((e as { code?: unknown }).code === "ENOENT") return null;
        throw e;
      }
    },
    scheduler,
    cpdHome: env["CPD_HOME"] ?? `${homedir()}/.cpd`,
  };
}

export function cpdWebPlugin(options?: { boundaries?: PluginBoundaries }): Plugin {
  let closeAll: (() => Promise<void>) | null = null;
  return {
    name: "cpd-web",
    apply: "serve",
    async configureServer(server: ViteDevServer) {
      const boundaries = options?.boundaries ?? realBoundaries(process.env);
      const services = createServices(boundaries);
      let closed = false;
      // The gate is the raw start promise: it rejects on a failed start so the endpoint surfaces the
      // failure on its error path instead of resolving a gate while no service runs (20.1). The dev
      // server still configures; the failure is logged here, not swallowed into a lying open gate.
      const started = services.cockpit.start();
      started.catch(() => undefined);
      // The dev server hosts the one WebSocket endpoint on its own httpServer (web/CONTRACT.md §15.1),
      // so Vite's HMR socket is untouched.
      const endpoint = server.httpServer === null
        ? null
        : createWsServer({
            newConnection: (send) => services.newConnection(send),
            server: createUpgradeFactory(server.httpServer as unknown as WebSocketUpgradeServer),
            scheduler: boundaries.scheduler,
            started: () => started,
            onError: (e: unknown) => server.config.logger.error(`cpd-web: ${e instanceof Error ? e.message : String(e)}`),
            path: WS_PATH,
          });
      closeAll = async () => {
        if (closed) return;
        closed = true;
        endpoint?.dispose();
        services.cockpit.stop();
      };
      server.httpServer?.once("close", () => { void closeAll?.(); });
      await started.catch((e: unknown) => {
        server.config.logger.error(`cpd-web: cockpit failed to start: ${e instanceof Error ? e.message : String(e)}`);
      });
    },
    closeBundle() {
      return closeAll?.();
    },
  };
}
