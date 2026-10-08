import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import {
  getMessages,
  openEngine,
  sseFrame,
  submitPrompt,
  getConversation,
  listConversations,
  forkPlan,
  ctx,
  getPlan,
  addPlanJob,
  patchPlanJob,
  deletePlanJob,
  runPlanJob,
  abortPlanJob,
  onJobOutput,
  UnknownConversationError,
  GraphError,
  DuplicateJobIdError,
  UnknownJobIdError,
  UnknownRunJobIdError,
  JobAlreadyRunningError,
  JobDependencyCycleError,
  type Engine,
} from "./engine.ts";
import { acquireLock, releaseLock } from "./lock.ts";
import { resolveConversationId, resolvePromptConversationId } from "./plans.ts";

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

  try {
    acquireLock(DB_PATH);
  } catch (error) {
    console.error((error as Error).message);
    process.exit(1);
  }

  const engine: Engine = await openEngine({ dbPath: DB_PATH });

  // SSE fan-out: each client watches one conversation id and is pushed `messages` on change,
  // plus every client gets `conversations` pushed when the conversation set changes.
  const sseClients = new Map<ServerResponse, number>();

  async function pushMessagesFor(conversationId: number): Promise<void> {
    const conversation = await getConversation(engine, conversationId);
    if (!conversation) return;
    const messages = await getMessages(conversation);
    const frame = sseFrame({ type: "messages", messages });
    for (const [client, watching] of sseClients) {
      if (watching === conversationId) client.write(frame);
    }
  }

  async function pushConversations(): Promise<void> {
    if (sseClients.size === 0) return;
    const conversations = await listConversations(engine);
    const frame = sseFrame({ type: "conversations", conversations });
    for (const client of sseClients.keys()) client.write(frame);
  }

  async function pushPlanFor(conversationId: number): Promise<void> {
    const jobs = await getPlan(engine, conversationId);
    const frame = sseFrame({ type: "plan", conversation: conversationId, jobs });
    for (const [client, watching] of sseClients) {
      if (watching === conversationId) client.write(frame);
    }
  }

  onJobOutput((_taskKey, jobId, conversationId, text) => {
    const frame = sseFrame({ type: "job-output", conversation: conversationId, jobId, text });
    for (const [client, watching] of sseClients) {
      if (watching === conversationId) client.write(frame);
    }
  });

  // Durable's `viewState().subscribe()` is the primary change-detection path; if that proves
  // unavailable at runtime we fall back to a 500ms poll (see catch below) that only pushes when
  // the root transcript actually changed, as the card allows. The conversation *set* is always
  // polled, since Durable has no dedicated "new conversation" event.
  let unsubscribe: (() => void) | undefined;
  let viewState: Awaited<ReturnType<Engine["root"]["viewState"]>> | undefined;
  let pollTimer: NodeJS.Timeout | undefined;
  try {
    const state = await engine.root.viewState(ctx);
    viewState = state;
    unsubscribe = state.subscribe(() => {
      void pushMessagesFor(engine.root.id);
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
          void pushMessagesFor(engine.root.id);
        }
      });
    }, 500);
  }

  let lastConversationCount = -1;
  const conversationPollTimer = setInterval(() => {
    void listConversations(engine).then((conversations) => {
      if (conversations.length !== lastConversationCount) {
        lastConversationCount = conversations.length;
        void pushConversations();
      }
    });
  }, 1000);

  const server = createServer((req, res) => {
    void handleRequest(req, res);
  });

  async function handleRequest(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const url = new URL(req.url ?? "/", "http://localhost");

    if (req.method === "GET" && url.pathname === "/api/health") {
      sendJson(res, 200, { ok: true, conversationId: engine.root.id });
      return;
    }

    if (req.method === "GET" && url.pathname === "/api/conversations") {
      const conversations = await listConversations(engine);
      sendJson(res, 200, { conversations });
      return;
    }

    if (req.method === "POST" && url.pathname === "/api/fork") {
      const raw = await readBody(req);
      let at: number;
      let title: string | undefined;
      try {
        const body = JSON.parse(raw) as { at?: unknown; title?: unknown };
        if (typeof body.at !== "number") throw new Error("at must be a number");
        at = body.at;
        if (body.title !== undefined) {
          if (typeof body.title !== "string") throw new Error("title must be a string");
          title = body.title;
        }
      } catch {
        sendJson(res, 400, { error: "Expected JSON body { at: number, title?: string }" });
        return;
      }
      const result = await forkPlan(engine, at, title);
      sendJson(res, 200, result);
      return;
    }

    if (req.method === "GET" && url.pathname === "/api/messages") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const conversation = await getConversation(engine, conversationId);
      if (!conversation) {
        sendJson(res, 404, { error: "Unknown conversation" });
        return;
      }
      const messages = await getMessages(conversation);
      sendJson(res, 200, { messages });
      return;
    }

    if (req.method === "POST" && url.pathname === "/api/prompt") {
      const raw = await readBody(req);
      let text: string;
      let bodyConversation: number | undefined;
      try {
        const body = JSON.parse(raw) as { text?: unknown; conversation?: unknown };
        if (typeof body.text !== "string") throw new Error("text must be a string");
        text = body.text;
        if (body.conversation !== undefined) {
          if (typeof body.conversation !== "number") throw new Error("conversation must be a number");
          bodyConversation = body.conversation;
        }
      } catch {
        sendJson(res, 400, { error: "Expected JSON body { text: string, conversation?: number }" });
        return;
      }
      const resolution = resolvePromptConversationId(url, bodyConversation, engine.root.id as unknown as number);
      if (resolution.kind === "conflict") {
        sendJson(res, 400, { error: "conversation query param and body disagree" });
        return;
      }
      const conversation = await getConversation(engine, resolution.id);
      if (!conversation) {
        sendJson(res, 404, { error: "Unknown conversation" });
        return;
      }
      const result = await submitPrompt(conversation, text);
      sendJson(res, 200, result);
      return;
    }

    if (req.method === "GET" && url.pathname === "/api/plan") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const conversation = await getConversation(engine, conversationId);
      if (!conversation) {
        sendJson(res, 404, { error: "Unknown conversation" });
        return;
      }
      const jobs = await getPlan(engine, conversationId);
      sendJson(res, 200, { jobs });
      return;
    }

    if (req.method === "POST" && url.pathname === "/api/plan/job") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const raw = await readBody(req);
      let title: string;
      let needs: string[] | undefined;
      let command: string | undefined;
      try {
        const body = JSON.parse(raw) as { title?: unknown; needs?: unknown; command?: unknown };
        if (typeof body.title !== "string") throw new Error("title must be a string");
        title = body.title;
        if (body.needs !== undefined) {
          if (!Array.isArray(body.needs) || !body.needs.every((n) => typeof n === "string")) {
            throw new Error("needs must be a string array");
          }
          needs = body.needs;
        }
        if (body.command !== undefined) {
          if (typeof body.command !== "string" || body.command.trim() === "") {
            throw new Error("command must be a non-empty string");
          }
          command = body.command;
        }
      } catch {
        sendJson(res, 400, {
          error: "Expected JSON body { title: string, needs?: string[], command?: string }",
        });
        return;
      }
      try {
        const job = await addPlanJob(engine, conversationId, { title, needs, command });
        void pushPlanFor(conversationId);
        sendJson(res, 200, { job });
      } catch (error) {
        if (error instanceof UnknownConversationError) {
          sendJson(res, 404, { error: error.message });
        } else if (error instanceof GraphError || error instanceof DuplicateJobIdError) {
          sendJson(res, 400, { error: error.message });
        } else {
          throw error;
        }
      }
      return;
    }

    if (req.method === "PATCH" && url.pathname === "/api/plan/job") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const raw = await readBody(req);
      let id: string;
      let status: "draft" | "queued" | "running" | "done" | "failed" | undefined;
      let title: string | undefined;
      let needs: string[] | undefined;
      let command: string | null | undefined;
      try {
        const body = JSON.parse(raw) as {
          id?: unknown;
          status?: unknown;
          title?: unknown;
          needs?: unknown;
          command?: unknown;
        };
        if (typeof body.id !== "string") throw new Error("id must be a string");
        id = body.id;
        if (body.status !== undefined) {
          if (typeof body.status !== "string") throw new Error("status must be a string");
          status = body.status as typeof status;
        }
        if (body.title !== undefined) {
          if (typeof body.title !== "string") throw new Error("title must be a string");
          title = body.title;
        }
        if (body.needs !== undefined) {
          if (!Array.isArray(body.needs) || !body.needs.every((n) => typeof n === "string")) {
            throw new Error("needs must be a string array");
          }
          needs = body.needs;
        }
        if (body.command !== undefined) {
          if (body.command === null) {
            command = null;
          } else if (typeof body.command !== "string" || body.command.trim() === "") {
            throw new Error("command must be a non-empty string or null");
          } else {
            command = body.command;
          }
        }
      } catch {
        sendJson(res, 400, {
          error:
            "Expected JSON body { id: string, status?: string, title?: string, needs?: string[], command?: string | null }",
        });
        return;
      }
      try {
        const job = await patchPlanJob(engine, conversationId, { id, status, title, needs, command });
        void pushPlanFor(conversationId);
        sendJson(res, 200, { job });
      } catch (error) {
        if (error instanceof UnknownConversationError || error instanceof UnknownJobIdError) {
          sendJson(res, 404, { error: error.message });
        } else if (error instanceof GraphError) {
          sendJson(res, 400, { error: error.message });
        } else {
          throw error;
        }
      }
      return;
    }

    if (req.method === "DELETE" && url.pathname === "/api/plan/job") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const raw = await readBody(req);
      let id: string;
      try {
        const body = JSON.parse(raw) as { id?: unknown };
        if (typeof body.id !== "string") throw new Error("id must be a string");
        id = body.id;
      } catch {
        sendJson(res, 400, { error: "Expected JSON body { id: string }" });
        return;
      }
      try {
        await deletePlanJob(engine, conversationId, id);
        void pushPlanFor(conversationId);
        sendJson(res, 200, { ok: true });
      } catch (error) {
        if (error instanceof UnknownConversationError) {
          sendJson(res, 404, { error: error.message });
        } else {
          throw error;
        }
      }
      return;
    }

    if (req.method === "POST" && url.pathname === "/api/plan/job/run") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const raw = await readBody(req);
      let id: string;
      try {
        const body = JSON.parse(raw) as { id?: unknown };
        if (typeof body.id !== "string") throw new Error("id must be a string");
        id = body.id;
      } catch {
        sendJson(res, 400, { error: "Expected JSON body { id: string }" });
        return;
      }
      try {
        const result = await runPlanJob(engine, conversationId, id);
        void pushPlanFor(conversationId);
        sendJson(res, 200, result);
      } catch (error) {
        if (error instanceof UnknownConversationError || error instanceof UnknownRunJobIdError) {
          sendJson(res, 404, { error: error.message });
        } else if (error instanceof JobAlreadyRunningError) {
          sendJson(res, 409, { error: error.message });
        } else if (error instanceof JobDependencyCycleError) {
          sendJson(res, 400, { error: error.message });
        } else {
          throw error;
        }
      }
      return;
    }

    if (req.method === "POST" && url.pathname === "/api/plan/job/abort") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const raw = await readBody(req);
      let id: string;
      try {
        const body = JSON.parse(raw) as { id?: unknown };
        if (typeof body.id !== "string") throw new Error("id must be a string");
        id = body.id;
      } catch {
        sendJson(res, 400, { error: "Expected JSON body { id: string }" });
        return;
      }
      try {
        await abortPlanJob(engine, conversationId, id);
        void pushPlanFor(conversationId);
        sendJson(res, 200, { ok: true });
      } catch (error) {
        if (error instanceof UnknownConversationError || error instanceof UnknownRunJobIdError) {
          sendJson(res, 404, { error: error.message });
        } else {
          throw error;
        }
      }
      return;
    }

    if (req.method === "GET" && url.pathname === "/api/stream") {
      const conversationId = resolveConversationId(url, engine.root.id as unknown as number);
      const conversation = await getConversation(engine, conversationId);
      if (!conversation) {
        sendJson(res, 404, { error: "Unknown conversation" });
        return;
      }

      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      });
      sseClients.set(res, conversationId);

      const messages = await getMessages(conversation);
      res.write(sseFrame({ type: "messages", messages }));
      const conversations = await listConversations(engine);
      res.write(sseFrame({ type: "conversations", conversations }));

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
    for (const client of sseClients.keys()) client.end();
    sseClients.clear();
    unsubscribe?.();
    viewState?.dispose();
    if (pollTimer) clearInterval(pollTimer);
    clearInterval(conversationPollTimer);
    server.close();
    await engine.close();
    releaseLock(DB_PATH);
    process.exit(0);
  }

  process.on("SIGINT", () => void shutdown("SIGINT"));
  process.on("SIGTERM", () => void shutdown("SIGTERM"));
  process.on("exit", () => releaseLock(DB_PATH));
}

main().catch((error) => {
  console.error("[cpd-daemon] fatal:", error);
  process.exit(1);
});
