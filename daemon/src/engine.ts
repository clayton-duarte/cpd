import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import {
  createRegistry,
  Harness,
  ROOT_CONVERSATION_ID,
  type Conversation,
  type ConversationId,
  type EntryId,
  type Storage,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { openNodeSqliteDatabase } from "@earendil-works/pi-durable/storage/sqlite/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { githubCopilotProvider } from "@earendil-works/pi-ai/providers/github-copilot";
import type { Message as PiMessage } from "@earendil-works/pi-ai";
import { FileCredentialStore } from "./credentials.ts";
import {
  deriveTitle,
  openTitleStore,
  shapeConversationTree,
  type ConversationNode,
  type RawConversationRecord,
  type TitleStore,
} from "./plans.ts";

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
  storage: Storage;
  titleStore: TitleStore;
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

  const titleDb = await openNodeSqliteDatabase(options.dbPath);
  const titleStore = await openTitleStore(titleDb);

  return {
    harness,
    root,
    storage,
    titleStore,
    async close() {
      await titleDb.close();
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

/** Look up a conversation handle by id, falling back to the root if not found. */
export async function getConversation(
  engine: Engine,
  id: number,
): Promise<Conversation | undefined> {
  if (id === (engine.root.id as unknown as number)) return engine.root;
  return engine.harness.conversation(id as unknown as ConversationId, ctx);
}

/** The first user message's text in a conversation, used to derive its title. */
async function firstUserMessageText(engine: Engine, id: number): Promise<string | undefined> {
  const conversation = await getConversation(engine, id);
  if (!conversation) return undefined;
  const view = await conversation.context(ctx, {});
  const messages = flattenMessages(view.messages);
  return messages.find((message) => message.role === "user")?.content;
}

/**
 * Resolve the title for one conversation id: a stored override wins, otherwise derive it from
 * the first user message (root is always "Lead").
 */
export async function titleForConversation(engine: Engine, id: number): Promise<string> {
  const stored = await engine.titleStore.get(id);
  if (stored) return stored;
  const isRoot = id === (engine.root.id as unknown as number);
  const firstMessage = isRoot ? undefined : await firstUserMessageText(engine, id);
  return deriveTitle(firstMessage, { isRoot });
}

/** List every conversation as the API's `ConversationNode` tree, following pagination. */
export async function listConversations(engine: Engine): Promise<ConversationNode[]> {
  const items: RawConversationRecord[] = [];
  let cursor: unknown;
  for (;;) {
    const page = await engine.storage.scanConversations({}, 100, cursor as never, ctx);
    for (const record of page.items) {
      items.push({
        id: record.id as unknown as number,
        parent: record.parent
          ? { conversationId: record.parent.conversationId as unknown as number, at: record.parent.at as unknown as number }
          : undefined,
      });
    }
    if (!page.next) break;
    cursor = page.next;
  }

  const titles = new Map<number, string>();
  for (const item of items) {
    titles.set(item.id, await titleForConversation(engine, item.id));
  }
  return shapeConversationTree(items, (id) => titles.get(id) ?? "Untitled");
}

/**
 * Fork a plan thread off the lead message `at` (an `EntryId` in the given conversation's
 * history). Ownership is mandatory per the verified recipe; `{ kind: "ownerless" }` is used for
 * a thread the lead created directly.
 */
export async function forkPlan(
  engine: Engine,
  at: number,
  title?: string,
): Promise<{ id: number }> {
  const plan = await engine.root.fork(
    at as EntryId,
    {
      ownership: { kind: "ownerless" },
      agent: {
        model: { provider: "github-copilot", modelId: "claude-opus-5" },
        instructions: "You are a CPD plan thread.",
      },
    },
    ctx,
  );
  if (title) {
    await engine.titleStore.set(plan.id as unknown as number, title);
  }
  return { id: plan.id as unknown as number };
}

export { ctx, ROOT_CONVERSATION_ID };
