import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, watch, type FSWatcher } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { connect, type Socket } from "node:net";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin, type PluginOption } from "vite";
import { createLiveHandler, type LiveDeps, type LiveSession, type InboxItem, type LivePlan } from "./src/api/live-server.ts";
import { createHostClient, type HostClient, type HostConnect } from "./src/api/host.ts";
import { createWsBridge } from "./src/api/ws-bridge.ts";
import { applyHostFrame, type HostState } from "./src/translate.ts";
import type { TranscriptMessage } from "./src/transcript.ts";

// The web client dev/build config (docs/research/web-client-api.md §6, A2): React + the
// Tailwind v4 plugin, and the dev server on port 5173 against the typed mock. With
// CPD_WEB_LIVE=1 it also mounts the dev live adapter (web/README.md); without it, nothing new.

/** The cpd binary: `$CPD_BIN`, else this repo's built binary, else `cpd` on PATH. */
function cpdBin(): string {
  if (process.env.CPD_BIN) return process.env.CPD_BIN;
  const built = fileURLToPath(new URL("../target/debug/cpd", import.meta.url));
  return existsSync(built) ? built : "cpd";
}

/** Runs one cpd command and resolves its stdout; a non-zero exit rejects. */
function runCpd(args: readonly string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cpdBin(), [...args], { maxBuffer: 64 * 1024 * 1024 }, (error, stdout) => {
      if (error) reject(error);
      else resolve(stdout);
    });
  });
}

/** The WebSocket path the bridge mounts on, and the handshake constant (RFC 6455). */
const WS_PATH = /^\/__cpd\/sessions\/([^/]+)\/ws$/;
const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/** The Sec-WebSocket-Accept value for a client key. */
function acceptKey(key: string): string {
  return createHash("sha1").update(key + WS_GUID).digest("base64");
}

/** One server→client text frame (the only frame kind the bridge sends). */
function encodeTextFrame(text: string): Buffer {
  const payload = Buffer.from(text, "utf8");
  const length = payload.length;
  if (length < 126) return Buffer.concat([Buffer.from([0x81, length]), payload]);
  if (length < 65536) {
    const header = Buffer.alloc(4);
    header[0] = 0x81;
    header[1] = 126;
    header.writeUInt16BE(length, 2);
    return Buffer.concat([header, payload]);
  }
  const header = Buffer.alloc(10);
  header[0] = 0x81;
  header[1] = 127;
  header.writeBigUInt64BE(BigInt(length), 2);
  return Buffer.concat([header, payload]);
}

/** Connects to a unix socket, resolving once it is open. */
const netConnect: HostConnect = (path: string): Promise<Socket> =>
  new Promise((resolve, reject) => {
    const socket = connect(path);
    socket.once("connect", () => resolve(socket));
    socket.once("error", reject);
  });

/** One `session list` row (only the fields realDeps reads). */
interface SessionRow {
  id?: unknown;
  role?: unknown;
  runner?: unknown;
  name?: unknown;
  state?: unknown;
  title?: unknown;
  context?: unknown;
  file?: unknown;
}

/** One `plan list` row (only the fields realDeps reads). */
interface PlanListRow {
  id?: unknown;
  context?: unknown;
  title?: unknown;
  phase?: unknown;
  lead_session?: unknown;
  archived_at?: unknown;
  counts?: unknown;
}

/** The six Job counts `cpd plan list` prints, coerced to numbers with a missing one at zero. */
function planCounts(value: unknown): LivePlan["counts"] {
  const counts = (value ?? {}) as Record<string, unknown>;
  const at = (key: string): number => (typeof counts[key] === "number" ? (counts[key] as number) : 0);
  return {
    draft: at("draft"),
    queued: at("queued"),
    running: at("running"),
    merged: at("merged"),
    failed: at("failed"),
    cancelled: at("cancelled"),
  };
}

/** The real deps plus the pi-file lookup the bridge's `fs.watch` needs. */
interface RealDeps extends LiveDeps {
  piFile(id: string): Promise<string | undefined>;
}

/**
 * A durable Session's transcript over its host socket, one-shot: attach, fold the first stream
 * frame, stop. Resolves `undefined` when no live host answers, so the caller falls back to `tail`.
 */
function readHostOnce(hostClient: HostClient, id: string): Promise<readonly TranscriptMessage[] | undefined> {
  return new Promise((resolve) => {
    let state: HostState = { messages: [], cursor: undefined };
    let settled = false;
    let stop: () => void = () => {};
    const finish = (value: readonly TranscriptMessage[] | undefined): void => {
      if (settled) return;
      settled = true;
      stop();
      resolve(value);
    };
    stop = hostClient.attach(
      id,
      (frame) => {
        if (frame.type === "snapshot" || frame.type === "events") {
          state = applyHostFrame(state, frame);
          finish(state.messages);
        } else if (frame.type === "bye") {
          finish(undefined);
        }
      },
      () => finish(undefined),
    );
  });
}

/** Watches a pi Session's file with `fs.watch` (the de-polled liveness signal), returning the stop closure. */
function watchPiFile(
  piFile: (id: string) => Promise<string | undefined>,
  id: string,
  onChange: () => void,
): () => void {
  let watcher: FSWatcher | undefined;
  void (async (): Promise<void> => {
    const file = await piFile(id);
    if (file === undefined) return;
    try {
      watcher = watch(file, () => onChange());
    } catch {
      // The file disappeared before the watch; the bridge simply sends nothing new.
    }
  })();
  return () => watcher?.close();
}

/** Builds the real `LiveDeps`: it runs the cpd CLI and reads the pi session file it names. */
function realDeps(hostClient: HostClient): RealDeps {
  // `session list` carries each Session's pi session file; the map saves a second call.
  const files = new Map<string, string>();

  const list = async (): Promise<readonly LiveSession[]> => {
    const parsed = JSON.parse(
      await runCpd(["session", "list", "--role", "lead", "--role", "manager"]),
    ) as { sessions?: SessionRow[] };
    const sessions: LiveSession[] = [];
    for (const row of parsed.sessions ?? []) {
      const runner = row.runner === "durable" ? "durable" : "pi";
      const id = String(row.id);
      if (typeof row.file === "string") files.set(id, row.file);
      const session: LiveSession = {
        id,
        role: row.role === "manager" ? "manager" : "lead",
        runner,
      };
      const title = typeof row.title === "string" ? row.title : row.name;
      if (typeof title === "string") session.title = title;
      if (typeof row.state === "string") session.state = row.state;
      if (typeof row.context === "string") session.context = row.context;
      if (typeof row.name === "string") session.name = row.name;
      sessions.push(session);
    }
    return sessions;
  };

  const piFile = async (id: string): Promise<string | undefined> => {
    const cached = files.get(id);
    if (cached !== undefined) return cached;
    const parsed = JSON.parse(await runCpd(["session", "show", id])) as {
      session?: { file?: unknown };
    };
    const file = parsed.session?.file;
    if (typeof file !== "string") return undefined;
    files.set(id, file);
    return file;
  };

  return {
    list,
    piFile,
    // The audit's minimal host-socket fix (Verdict 2): the durable read prefers the live socket.
    host: (id) => readHostOnce(hostClient, id),
    async readPi(id) {
      const file = await piFile(id);
      if (file === undefined) return undefined;
      try {
        return await readFile(file, "utf8");
      } catch {
        return undefined;
      }
    },
    async tail(id) {
      return await runCpd(["session", "tail", id]);
    },
    async send(id, text, unblocks) {
      await runCpd(["session", "send", id, text, "--unblocks", unblocks]);
    },
    // The durable send over the host socket's `submit` (web/README.md "Send", audit Verdict 4).
    async durableSend(id, text) {
      await hostClient.submit?.(id, text, "followUp");
    },
    async stat(id) {
      const file = await piFile(id);
      if (file === undefined) return undefined;
      try {
        return (await stat(file)).mtimeMs;
      } catch {
        return undefined;
      }
    },
    async inbox() {
      const parsed = JSON.parse(await runCpd(["inbox", "list"])) as { items?: readonly InboxItem[] };
      return parsed.items ?? [];
    },
    async plans() {
      // The CLI's own rows; an archived Plan is never drawn (web/README.md "Context tree rows").
      const parsed = JSON.parse(await runCpd(["plan", "list"])) as { plans?: readonly PlanListRow[] };
      const plans: LivePlan[] = [];
      for (const row of parsed.plans ?? []) {
        if (row.archived_at !== null && row.archived_at !== undefined) continue;
        plans.push({
          id: String(row.id),
          context: String(row.context ?? ""),
          title: String(row.title ?? ""),
          phase: String(row.phase ?? ""),
          lead_session: typeof row.lead_session === "string" ? row.lead_session : null,
          archived_at: null,
          counts: planCounts(row.counts),
        });
      }
      return plans;
    },
  };
}

/** Mounts the dev live adapter's `/__cpd/` middleware and the WebSocket bridge; only with CPD_WEB_LIVE=1. */
function liveAdapterPlugin(): Plugin {
  return {
    name: "cpd-live-adapter",
    apply: "serve",
    configureServer(server) {
      const hostClient = createHostClient(netConnect);
      const deps = realDeps(hostClient);
      const handler = createLiveHandler(deps);
      server.middlewares.use((req, res, next) => {
        handler(req, res).then(
          (matched) => {
            if (!matched) next();
          },
          (error: unknown) => next(error instanceof Error ? error : new Error(String(error))),
        );
      });

      const expectedToken = (): string => process.env.CPD_WEB_TOKEN ?? "dev-token";
      const bridge = createWsBridge({
        token: expectedToken,
        session: async (id) => (await deps.list()).find((candidate) => candidate.id === id),
        host: hostClient,
        readPi: deps.readPi,
        watchPi: (id, onChange) => watchPiFile(deps.piFile, id, onChange),
        tail: deps.tail,
      });

      // The token is checked BEFORE the upgrade: a missing or wrong token refuses with 401 and the
      // socket never opens (web/README.md "The route").
      server.httpServer?.on("upgrade", (req, socket) => {
        const url = new URL(req.url ?? "/", "http://localhost");
        const match = WS_PATH.exec(url.pathname);
        if (match === null) return;
        if (url.searchParams.get("token") !== expectedToken()) {
          socket.write("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
          socket.destroy();
          return;
        }
        const key = req.headers["sec-websocket-key"];
        const value = Array.isArray(key) ? key[0] : key;
        if (typeof value !== "string") {
          socket.destroy();
          return;
        }
        socket.write(
          "HTTP/1.1 101 Switching Protocols\r\n" +
            "Upgrade: websocket\r\n" +
            "Connection: Upgrade\r\n" +
            `Sec-WebSocket-Accept: ${acceptKey(value)}\r\n\r\n`,
        );
        const id = decodeURIComponent(match[1] ?? "");
        const close = bridge.handle({
          id,
          token: expectedToken(),
          send: (frame) => {
            if (!socket.destroyed) socket.write(encodeTextFrame(JSON.stringify(frame)));
          },
        });
        socket.on("close", close);
        socket.on("error", close);
      });
    },
  };
}

// The live adapter is opt-in: exactly CPD_WEB_LIVE=1 mounts it; anything else (including unset)
// leaves the existing dev and build flows exactly as they were.
const plugins: PluginOption[] = [react(), tailwindcss()];
if (process.env.CPD_WEB_LIVE === "1") plugins.push(liveAdapterPlugin());

export default defineConfig({
  plugins,
  server: {
    port: 5173,
    strictPort: true,
  },
});
