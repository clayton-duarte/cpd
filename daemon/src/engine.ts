import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { createRegistry, Harness, ROOT_CONVERSATION_ID, type Conversation } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { githubCopilotProvider } from "@earendil-works/pi-ai/providers/github-copilot";
import type { Message as PiMessage } from "@earendil-works/pi-ai";
import { FileCredentialStore } from "./credentials.ts";

export type Message = {
  role: "user" | "assistant" | "system";
  content: string;
};

/**
 * Flatten Durable/pi-ai's content-array messages into the plain `{role, content}` shape the HTTP
 * API exposes. Text parts are joined; non-text parts (tool calls, images, thinking) are dropped.
 * Empty system messages (no text content at all) are skipped entirely.
 */
export function flattenMessages(messages: readonly PiMessage[]): Message[] {
  const out: Message[] = [];
  for (const message of messages) {
    if (message.role === "toolResult") continue;
    const role = message.role as "user" | "assistant" | "system";
    let content: string;
    if (typeof message.content === "string") {
      content = message.content;
    } else {
      content = message.content
        .filter((part): part is { type: "text"; text: string } => part.type === "text")
        .map((part) => part.text)
        .join("");
    }
    if (role === "system" && content === "") continue;
    out.push({ role, content });
  }
  return out;
}

/** Serialize an SSE payload to exactly `data: <json>\n\n`. */
export function sseFrame(payload: unknown): string {
  return `data: ${JSON.stringify(payload)}\n\n`;
}

export type Engine = {
  harness: Harness;
  root: Conversation;
  close(): Promise<void>;
};

export type OpenEngineOptions = {
  dbPath: string;
  authPath?: string;
};

/**
 * Open the Pi Durable harness over the given SQLite file, registering the GitHub Copilot
 * provider and resuming scheduling. Follows the verified recipe in the F1 card exactly: no
 * `providers` option on `createModels`, explicit `setProvider`, mandatory `resume()`,
 * `modelId` (not `id`), and `{ type: "input", content }` (not `{ text }`) on submit.
 */
export async function openEngine(options: OpenEngineOptions): Promise<Engine> {
  const credentials = new FileCredentialStore(options.authPath);

  // Fail fast with an actionable message rather than letting a later, unrelated error surface.
  const existing = await credentials.read("github-copilot");
  if (!existing) {
    throw new Error(
      `No GitHub Copilot credentials found at ${options.authPath ?? "the default pi auth path"}. Run \`pi\` and log in first.`,
    );
  }

  const models = createModels({ credentials });
  models.setProvider(githubCopilotProvider());

  const storage = await openNodeSqliteStorage(options.dbPath);
  const harness = await Harness.open(
    storage,
    {
      models,
      registry: createRegistry(),
      onReport: (error) => console.error("[cpd-daemon] harness report:", error),
    },
    ctx,
  );
  harness.resume();

  const root = await harness.root(ctx, {
    agent: {
      model: { provider: "github-copilot", modelId: "claude-opus-5" },
      instructions: "You are the CPD lead agent.",
    },
  });

  return {
    harness,
    root,
    async close() {
      await harness.close(ctx);
    },
  };
}

export async function getMessages(root: Conversation): Promise<Message[]> {
  const view = await root.context(ctx, {});
  return flattenMessages(view.messages);
}

export async function submitPrompt(
  root: Conversation,
  text: string,
): Promise<{ status: "done" | "unanswered"; reason?: string }> {
  const submission = await root.submit({ type: "input", content: text }, ctx);
  const settled = await submission.wait(ctx);
  if (settled.status === "unanswered") {
    console.error("[cpd-daemon] submission unanswered:", settled.reason);
  }
  return { status: settled.status, reason: settled.reason };
}

export { ctx, ROOT_CONVERSATION_ID };
