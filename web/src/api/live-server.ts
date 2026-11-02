// The dev live-adapter /__cpd/ route handler (web/README.md "`web/src/api/live-server.ts`",
// hs-d819). Framework-free and dependency-injected: every external boundary is a `LiveDeps`
// function, so no test spawns cpd, opens a socket or starts a server.
import type { IncomingMessage, ServerResponse } from "node:http";
import { parsePiSession, parseTailOutput, toSnapshotFrame, type TranscriptMessage } from "../transcript.ts";
import type { InboxItem } from "./readmodel.ts";

/** The Inbox item shape `cpd inbox list` prints, the rail's row source (web/README.md "The live Inbox in the right rail"). */
export type { InboxItem };

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
  /** The read's `asleep_at`: set means the Session was put to sleep (web/README.md "Context tree rows"), whatever its `state` string says. */
  asleep_at?: string | null;
  /** The Context the Session belongs to (`cpd session list` row's `context`); the tree groups by it. */
  context?: string;
  /** The Session's display name (`cpd session list` row's `name`); the tree labels rows with it. */
  name?: string;
}

/** One Plan's Job counts as `cpd plan list` prints them (web/README.md "Where the numbers come from"). */
export interface LivePlanCounts {
  draft: number;
  queued: number;
  running: number;
  merged: number;
  failed: number;
  cancelled: number;
}

/**
 * One `cpd plan list` row the plans read answers (web/README.md "Context tree rows"): the Plan's
 * key (`id`), its Context, its Lead Session and its Job counts. The rail draws a Plan row from it.
 */
export interface LivePlan {
  id: string;
  context: string;
  title: string;
  phase: string;
  lead_session: string | null;
  archived_at: string | null;
  counts: LivePlanCounts;
}

/**
 * One Context the contexts read answers (web/README.md "Context tree rows"): the `cpd context
 * list` row's id and whether the Context is archived. The rail's Context tree reads them.
 */
export interface LiveContext {
  id: string;
  archived: boolean;
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
  /** The open Inbox items, from `cpd inbox list`; the rail renders them unchanged (web/README.md "The live Inbox in the right rail"). Optional so the handler's other routes build without it. */
  inbox?(): Promise<readonly InboxItem[]>;
  /** The Plans, from `cpd plan list`; the rail's Plan tree reads them (web/README.md "Where the numbers come from"). Optional so the handler's other routes build without it. */
  plans?(): Promise<readonly LivePlan[]>;
  /** The Contexts, from `cdp context list`; the rail's Context tree reads them (web/README.md "Context tree rows"). Optional so the handler's other routes build without it. */
  contexts?(): Promise<readonly LiveContext[]>;
}

/** Handles one request; resolves true when it matched a /__cpd/ route (the caller stops), false to pass through. */
export type LiveHandler = (req: IncomingMessage, res: ServerResponse) => Promise<boolean>;

/** The route that starts a /__cpd/ session path: transcript, frames or send. */
const SESSION_ROUTE = /^\/__cpd\/sessions\/([^/]+)\/(transcript|frames|send)$/;
/** The token from the `x-cpd-token` header or the `?token=` query parameter (either is accepted). */
const TOKEN_QUERY = "token";
/** How often the frames stream checks for a change. */
const POLL_MS = 1000;
/**
 * How many of the newest messages one `frames` snapshot carries. A whole pi transcript reaches
 * tens of megabytes, and Chrome's `EventSource` silently drops an event that large (measured
 * 2026-11-01: a 20 MB single `data:` line never fired `onmessage`, a 53-byte one did). The live
 * frame is the tail; older turns arrive through the transcript route's paging.
 */
const SNAPSHOT_TAIL = 40;
/**
 * The bytes one `frames` snapshot may hold, oldest turns dropped first. Measured on Chrome
 * 2026-11-01: a 1 MB and a 2 MB event fired `onmessage`, a 4 MB, 8 MB and 20 MB one never did.
 * The budget sits at half the smallest size known to fail, and the dropped turns are a paged read
 * away, so nothing is lost.
 */
const SNAPSHOT_BUDGET_BYTES = 1_000_000;
/** The most messages one paged transcript read returns. */
const PAGE_MAX = 500;
/** The body limits of contract 10.13. */
const TEXT_MAX = 20_000;
const UNBLOCKS_MAX = 200;

/** The newest turns within `SNAPSHOT_TAIL` and `SNAPSHOT_BUDGET_BYTES`; never nothing. */
function snapshotTail(messages: readonly TranscriptMessage[]): readonly TranscriptMessage[] {
  const tail: TranscriptMessage[] = [];
  let bytes = 0;
  for (let at = messages.length - 1; at >= 0 && tail.length < SNAPSHOT_TAIL; at -= 1) {
    const message = messages[at]!;
    const size = JSON.stringify(message).length;
    // The newest turn goes in whatever its size; an empty snapshot would blank the Chat.
    if (tail.length > 0 && bytes + size > SNAPSHOT_BUDGET_BYTES) break;
    bytes += size;
    tail.unshift(message);
  }
  return tail;
}

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

  const handleTranscript = async (res: ServerResponse, id: string, url: URL): Promise<void> => {
    const session = (await deps.list()).find((candidate) => candidate.id === id);
    if (session === undefined) {
      sendJson(res, 404, { ok: false, error: "session not found" });
      return;
    }
    const all = await readTranscript(session);
    // No paging asked for: the whole transcript, as before.
    if (!url.searchParams.has("limit")) {
      sendJson(res, 200, { ok: true, session: id, messages: all });
      return;
    }
    const limit = Math.min(
      Math.max(Number(url.searchParams.get("limit") ?? "") || SNAPSHOT_TAIL, 1),
      PAGE_MAX,
    );
    // `before` is the oldest id the client holds; the page ends just above it. An unknown id
    // (a compacted transcript, a restarted adapter) answers the newest page rather than guessing.
    const before = url.searchParams.get("before");
    const at = before === null ? all.length : all.findIndex((message) => message.id === before);
    const end = at < 0 ? all.length : at;
    const start = Math.max(0, end - limit);
    sendJson(res, 200, {
      ok: true,
      session: id,
      messages: all.slice(start, end),
      more: start > 0,
    });
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
    // The frame carries the newest turns only: a whole transcript is too large for EventSource.
    const writeFrame = (messages: readonly TranscriptMessage[]): void => {
      seq += 1;
      const tail = snapshotTail(messages);
      res.write(`data: ${JSON.stringify(toSnapshotFrame(tail, "live", seq))}\n\n`);
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
    if (path === "/__cpd/inbox" && method === "GET") {
      sendJson(res, 200, { ok: true, items: (await deps.inbox?.()) ?? [] });
      return true;
    }
    if (path === "/__cpd/plans" && method === "GET") {
      // The read is optional: an adapter with no plans wiring answers an empty list, so the
      // rail falls back to its Session tree (web/README.md "The fallback"). Every Plan of every
      // Context is answered, archived ones included, each row carrying its `archived` flag from
      // the CLI's `archived_at` (web/README.md "Context tree rows").
      const plans = (await deps.plans?.()) ?? [];
      sendJson(res, 200, {
        ok: true,
        plans: plans.map((plan) => ({ ...plan, archived: plan.archived_at !== null })),
      });
      return true;
    }
    if (path === "/__cpd/contexts" && method === "GET") {
      // The read is optional: an adapter with no contexts wiring answers an empty list, so the
      // rail falls back to its Session tree (web/README.md "The fallback").
      sendJson(res, 200, { ok: true, contexts: (await deps.contexts?.()) ?? [] });
      return true;
    }

    const match = SESSION_ROUTE.exec(path);
    if (match !== null) {
      const id = decodeURIComponent(match[1] ?? "");
      const action = match[2];
      if (action === "transcript" && method === "GET") {
        await handleTranscript(res, id, url);
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
