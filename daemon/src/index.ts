import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { getMessages, openEngine, sseFrame, submitPrompt, ctx, type Engine } from "./engine.ts";

const PORT = Number(process.env.CPD_DAEMON_PORT ?? 4317);
const DB_PATH = process.env.CPD_DB ?? ".cpd/cpd.sqlite";
const KEEPALIVE_MS = 30_000;

async function readBody(req: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

function sendJson(res: ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(payload);
}

async function main(): Promise<void> {
  await mkdir(dirname(DB_PATH), { recursive: true });

  const engine: Engine = await openEngine({ dbPath: DB_PATH });

  // SSE fan-out: every connected client gets pushed the full transcript whenever it changes.
  const sseClients = new Set<ServerResponse>();

  async function pushMessages(): Promise<void> {
    if (sseClients.size === 0) return;
    const messages = await getMessages(engine.root);
    const frame = sseFrame({ type: "messages", messages });
    for (const client of sseClients) client.write(frame);
  }

  // Durable's `viewState().subscribe()` is the primary change-detection path; if that proves
  // unavailable at runtime we fall back to a 500ms poll (see catch below) that only pushes when
  // the transcript actually changed, as the card allows.
  let unsubscribe: (() => void) | undefined;
  let pollTimer: NodeJS.Timeout | undefined;
  try {
    const state = await engine.root.viewState(ctx);
    unsubscribe = state.subscribe(() => {
      void pushMessages();
    });
  } catch (error) {
    console.error("[cpd-daemon] viewState().subscribe() unavailable, falling back to polling:", error);
    let lastCount = -1;
    let lastContent = "";
    pollTimer = setInterval(() => {
      void getMessages(engine.root).then((messages) => {
        const last = messages[messages.length - 1];
        const content = last?.content ?? "";
        if (messages.length !== lastCount || content !== lastContent) {
          lastCount = messages.length;
          lastContent = content;
          void pushMessages();
        }
      });
    }, 500);
  }

  const server = createServer((req, res) => {
    void handleRequest(req, res);
  });

  async function handleRequest(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const url = new URL(req.url ?? "/", "http://localhost");

    if (req.method === "GET" && url.pathname === "/api/health") {
      sendJson(res, 200, { ok: true, conversationId: engine.root.id });
      return;
    }

    if (req.method === "GET" && url.pathname === "/api/messages") {
      const messages = await getMessages(engine.root);
      sendJson(res, 200, { messages });
      return;
    }

    if (req.method === "POST" && url.pathname === "/api/prompt") {
      const raw = await readBody(req);
      let text: string;
      try {
        const body = JSON.parse(raw) as { text?: unknown };
        if (typeof body.text !== "string") throw new Error("text must be a string");
        text = body.text;
      } catch {
        sendJson(res, 400, { error: "Expected JSON body { text: string }" });
        return;
      }
      const result = await submitPrompt(engine.root, text);
      sendJson(res, 200, result);
      return;
    }

    if (req.method === "GET" && url.pathname === "/api/stream") {
      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      });
      sseClients.add(res);

      const messages = await getMessages(engine.root);
      res.write(sseFrame({ type: "messages", messages }));

      const keepalive = setInterval(() => {
        res.write(": keepalive\n\n");
      }, KEEPALIVE_MS);

      req.on("close", () => {
        clearInterval(keepalive);
        sseClients.delete(res);
      });
      return;
    }

    sendJson(res, 404, { error: "Not found" });
  }

  server.listen(PORT, () => {
    console.log(`[cpd-daemon] listening on http://localhost:${PORT}`);
  });

  async function shutdown(signal: string): Promise<void> {
    console.log(`[cpd-daemon] received ${signal}, shutting down`);
    for (const client of sseClients) client.end();
    sseClients.clear();
    unsubscribe?.();
    if (pollTimer) clearInterval(pollTimer);
    server.close();
    await engine.close();
    process.exit(0);
  }

  process.on("SIGINT", () => void shutdown("SIGINT"));
  process.on("SIGTERM", () => void shutdown("SIGTERM"));
}

main().catch((error) => {
  console.error("[cpd-daemon] fatal:", error);
  process.exit(1);
});
