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
  PlanDoc,
  addJob,
  removeJob,
  setStatus,
  validateGraph,
  DuplicateJobIdError,
  UnknownJobIdError,
  type Job,
  type JobStatus,
} from "./plans.ts";

export type Message = {
  id: number;
  role: "user" | "assistant" | "system";
  content: string;
};

/** Minimal shape consumed from a Durable `EntryRecord`: its id plus the `model` messages it
 * contributed, if any. Narrower than the full `EntryRecord` so fixtures in tests don't need to
 * fabricate every field. */
type MessageEntry = {
  readonly id: number;
  readonly model?: readonly PiMessage[];
};

/**
 * Flatten Durable's entries into the plain `{id, role, content}` shape the HTTP API exposes.
 * Walks `view.entries` (not `view.messages`) so every message carries the entry id it came from --
 * ids are sparse and never a position/array index. Text parts are joined; non-text parts (tool
 * calls, images, thinking) are dropped. Empty system messages (no text content at all) are
 * skipped entirely, as is any `toolResult` message. Several messages from one entry legitimately
 * share an id; callers must key by `${id}:${index}`, not the bare id.
 */
export function flattenMessages(entries: readonly MessageEntry[]): Message[] {
  const out: Message[] = [];
  for (const entry of entries) {
    for (const message of entry.model ?? []) {
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
      out.push({ id: entry.id, role, content });
    }
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
  return flattenMessages(view.entries);
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
  const messages = flattenMessages(view.entries);
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

// --- H4: cpd.plan document CRUD, scoped per conversation (D112) -----------------------------

export type { Job, JobStatus };
export { DuplicateJobIdError, UnknownJobIdError };

export class UnknownConversationError extends Error {
  readonly id: number;
  constructor(id: number) {
    super(`Unknown conversation: ${id}`);
    this.id = id;
    this.name = "UnknownConversationError";
  }
}

/** Read the job list for one conversation's plan. Undefined conversation -> `UnknownConversationError`. */
export async function getPlan(engine: Engine, conversationId: number): Promise<Job[]> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);
  const state = await engine.harness.snapshot(PlanDoc, conversation.id, ctx);
  return state?.jobs ? [...state.jobs] : [];
}

/** A write that would create a cycle or dangling edge; carries the offending ids for the 400 body. */
export class GraphError extends Error {
  readonly detail: ReturnType<typeof validateGraph>;
  constructor(detail: ReturnType<typeof validateGraph>) {
    super(
      detail?.kind === "cycle"
        ? `Cycle detected: ${detail.ids.join(" -> ")}`
        : detail?.kind === "dangling"
          ? `Job ${detail.jobId} needs unknown job ${detail.missingId}`
          : "Invalid job graph",
    );
  }
}

/** Commit `nextJobs` into the conversation's plan doc after validating the graph; throws
 * `GraphError` and leaves the document untouched when the write would create a cycle or a
 * dangling edge. */
async function commitJobs(engine: Engine, conversationId: number, nextJobs: Job[]): Promise<void> {
  const invalid = validateGraph(nextJobs);
  if (invalid) throw new GraphError(invalid);

  await engine.harness.commit(async (tx) => {
    const draft = await tx.doc(PlanDoc, conversationId as unknown as ConversationId);
    draft.jobs = nextJobs;
  }, ctx);
}

/** Add a new job (status `draft`) to the conversation's plan, generating its id server-side. */
export async function addPlanJob(
  engine: Engine,
  conversationId: number,
  input: { title: string; needs?: string[] },
): Promise<Job> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);

  const current = await getPlan(engine, conversationId);
  const job: Job = { id: crypto.randomUUID(), title: input.title, status: "draft", needs: input.needs ?? [] };
  const nextJobs = addJob(current, job);
  await commitJobs(engine, conversationId, nextJobs);
  return job;
}

/** Patch an existing job's title/status/needs. Throws `UnknownJobIdError` for an unknown id. */
export async function patchPlanJob(
  engine: Engine,
  conversationId: number,
  patch: { id: string; status?: JobStatus; title?: string; needs?: string[] },
): Promise<Job> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);

  let current = await getPlan(engine, conversationId);
  if (!current.some((job) => job.id === patch.id)) {
    throw new UnknownJobIdError(patch.id);
  }

  if (patch.status !== undefined) {
    current = setStatus(current, patch.id, patch.status);
  }
  if (patch.title !== undefined || patch.needs !== undefined) {
    current = current.map((job) =>
      job.id === patch.id
        ? { ...job, title: patch.title ?? job.title, needs: patch.needs ?? job.needs }
        : job,
    );
  }

  await commitJobs(engine, conversationId, current);
  return current.find((job) => job.id === patch.id)!;
}

/** Delete a job from the conversation's plan, stripping it from every other job's `needs`. */
export async function deletePlanJob(engine: Engine, conversationId: number, id: string): Promise<void> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);

  const current = await getPlan(engine, conversationId);
  const nextJobs = removeJob(current, id);
  await commitJobs(engine, conversationId, nextJobs);
}
