// The dev live-adapter /__cpd/ route handler (web/README.md "`web/src/api/live-server.ts`",
// hs-d819). Framework-free and dependency-injected: every external boundary is a `LiveDeps`
// function, so no test spawns cpd, opens a socket or starts a server.
import type { IncomingMessage, ServerResponse } from "node:http";
import { parsePiSession, parseTailOutput, toSnapshotFrame, type TranscriptMessage } from "../transcript.ts";

/**
 * Reads a durable Session's transcript over its host socket (the `chat-host` read, web/README.md "The
 * durable host-socket read"). It resolves `undefined` when no live host answers — no socket file, a
 * refused connect, a refused handshake — so the handler falls back to `cpd session tail`.
 */
export type DurableHostRead = (id: string) => Promise<readonly TranscriptMessage[] | undefined>;

/** One Session the tree offers. */
export interface LiveSession {
  id: string;
  role: "lead" | "manager";
  runner: "pi" | "durable";
  title?: string;
  state?: string;
  /** The Context the Session belongs to (`cpd session list` row's `context`); the tree groups by it. */
  context?: string;
  /** The Session's display name (`cpd session list` row's `name`); the tree labels rows with it. */
  name?: string;
}

/** Every external boundary is injected; the handler itself touches nothing real. */
export interface LiveDeps {
  /** The lead and manager Sessions, from `cpd session list --role lead --role manager`. */
  list(): Promise<readonly LiveSession[]>;
  /** A pi Session's session file, as its full JSONL text; undefined when there is none (a durable Session, or an unknown id). */
  readPi(id: string): Promise<string | undefined>;
  /** A durable Session's transcript: the full stdout of `cpd session tail <id>`. Called only for runner "durable", and only as the fallback of the host-socket read (below). */
  tail(id: string): Promise<string>;
  /** The durable Session's host-socket read; absent, or `undefined` for a Session with no live host, means `tail(id)` is used. */
  host?: DurableHostRead;
  /** One send: `cpd session send <id> <text> --unblocks <reason>`. The only mutation there is. */
  send(id: string, text: string, unblocks: string): Promise<void>;
  /** A pi Session's file mtime in ms, undefined when absent: the frames stream's change signal. */
  stat(id: string): Promise<number | undefined>;
}

/** Handles one request; resolves true when it matched a /__cpd/ route (the caller stops), false to pass through. */
export type LiveHandler = (req: IncomingMessage, res: ServerResponse) => Promise<boolean>;

/** The route that starts a /__cpd/ session path: transcript, frames or send. */
const SESSION_ROUTE = /^\/__cpd\/sessions\/([^/]+)\/(transcript|frames|send)$/;
/** The token from the `x-cpd-token` header or the `?token=` query parameter (either is accepted). */
const TOKEN_QUERY = "token";
/** How often the frames stream checks for a change. */
const POLL_MS = 1000;
/** The body limits of contract 10.13. */
const TEXT_MAX = 20_000;
const UNBLOCKS_MAX = 200;

/** The configured token; `dev-token` until `CPD_WEB_TOKEN` says otherwise. */
function expectedToken(): string {
  return process.env.CPD_WEB_TOKEN ?? "dev-token";
}

/** The request's token, from the header or the query. */
function requestToken(req: IncomingMessage, url: URL): string | undefined {
  const header = req.headers["x-cpd-token"];
  const fromHeader = Array.isArray(header) ? header[0] : header;
  return fromHeader ?? url.searchParams.get(TOKEN_QUERY) ?? undefined;
}

/** Writes one JSON response. */
function sendJson(res: ServerResponse, status: number, payload: unknown): void {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(payload));
}

/** Reads a request body as UTF-8 text. */
async function readBody(req: IncomingMessage): Promise<string> {
  let body = "";
  for await (const chunk of req) {
    body += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf8");
  }
  return body;
}

/** Builds the injected-route handler. */
export function createLiveHandler(deps: LiveDeps): LiveHandler {
  const readDurable = async (session: LiveSession): Promise<readonly TranscriptMessage[]> => {
    // The host-socket read of the `chat-host` PR: prefer the live host (`deps.host`); a host that
    // does not answer (no socket, a refused connect or handshake) falls back to `cpd session tail`.
    if (deps.host !== undefined) {
      const messages = await deps.host(session.id);
      if (messages !== undefined) return messages;
    }
    return parseTailOutput(await deps.tail(session.id));
  };

  const readTranscript = async (session: LiveSession): Promise<readonly TranscriptMessage[]> => {
    if (session.runner === "pi") {
      const jsonl = await deps.readPi(session.id);
      return jsonl === undefined ? [] : parsePiSession(jsonl);
    }
    return readDurable(session);
  };

  const handleTranscript = async (res: ServerResponse, id: string): Promise<void> => {
    const session = (await deps.list()).find((candidate) => candidate.id === id);
    if (session === undefined) {
      sendJson(res, 404, { ok: false, error: "session not found" });
      return;
    }
    sendJson(res, 200, { ok: true, session: id, messages: await readTranscript(session) });
  };

  const handleSend = async (req: IncomingMessage, res: ServerResponse, id: string): Promise<void> => {
    let body: unknown;
    try {
      body = JSON.parse(await readBody(req));
    } catch {
      sendJson(res, 400, { ok: false, error: "invalid body" });
      return;
    }
    const { text, unblocks } = (body ?? {}) as { text?: unknown; unblocks?: unknown };
    if (
      typeof text !== "string" ||
      text.length < 1 ||
      text.length > TEXT_MAX ||
      typeof unblocks !== "string" ||
      unblocks.length < 1 ||
      unblocks.length > UNBLOCKS_MAX
    ) {
      sendJson(res, 400, { ok: false, error: "invalid body" });
      return;
    }
    await deps.send(id, text, unblocks);
    sendJson(res, 200, { ok: true });
  };

  const handleFrames = async (res: ServerResponse, id: string): Promise<boolean> => {
    const session = (await deps.list()).find((candidate) => candidate.id === id);
    if (session === undefined) {
      sendJson(res, 404, { ok: false, error: "session not found" });
      return true;
    }

    res.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
    });

    let seq = 0;
    const writeFrame = (messages: readonly TranscriptMessage[]): void => {
      seq += 1;
      res.write(`data: ${JSON.stringify(toSnapshotFrame(messages, "live", seq))}\n\n`);
    };

    // The change signal is read before the first snapshot, so a change after it is seen.
    let lastMtime = session.runner === "pi" ? await deps.stat(session.id) : undefined;
    let sent = await readTranscript(session);
    writeFrame(sent);

    return await new Promise<boolean>((resolve) => {
      let closed = false;
      let timer: ReturnType<typeof setInterval> | undefined;
      const finish = (): void => {
        if (closed) return;
        closed = true;
        if (timer !== undefined) clearInterval(timer);
        resolve(true);
      };
      res.on("close", finish);
      res.on("end", finish);

      let ticking = false;
      const tick = async (): Promise<void> => {
        if (closed || ticking) return;
        ticking = true;
        try {
          if (session.runner === "pi") {
            const mtime = await deps.stat(session.id);
            if (mtime !== lastMtime) {
              lastMtime = mtime;
              sent = await readTranscript(session);
              if (!closed) writeFrame(sent);
            }
          } else {
            const next = await readTranscript(session);
            if (JSON.stringify(next) !== JSON.stringify(sent)) {
              sent = next;
              if (!closed) writeFrame(sent);
            }
          }
        } finally {
          ticking = false;
        }
      };
      timer = setInterval(() => {
        void tick();
      }, POLL_MS);
    });
  };

  return async (req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    const path = url.pathname;
    // Only /__cpd/ routes are ours; anything else passes through untouched.
    if (!path.startsWith("/__cpd/")) return false;

    // Token check first: 401 before any read or send.
    if (requestToken(req, url) !== expectedToken()) {
      sendJson(res, 401, { ok: false, error: "missing or wrong token" });
      return true;
    }

    const method = req.method ?? "GET";
    if (path === "/__cpd/health" && method === "GET") {
      sendJson(res, 200, { ok: true });
      return true;
    }
    if (path === "/__cpd/sessions" && method === "GET") {
      sendJson(res, 200, { ok: true, sessions: await deps.list() });
      return true;
    }

    const match = SESSION_ROUTE.exec(path);
    if (match !== null) {
      const id = decodeURIComponent(match[1] ?? "");
      const action = match[2];
      if (action === "transcript" && method === "GET") {
        await handleTranscript(res, id);
        return true;
      }
      if (action === "send" && method === "POST") {
        await handleSend(req, res, id);
        return true;
      }
      if (action === "frames" && method === "GET") {
        return await handleFrames(res, id);
      }
    }

    return false;
  };
}
