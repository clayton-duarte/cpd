import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import {
  createRegistry,
  Harness,
  ROOT_CONVERSATION_ID,
  InboxDoc,
  AgentDoc,
  configure,
  type Conversation,
  type ConversationId,
  type EntryId,
  type ModelRef,
  type Storage,
  type TaskId,
} from "@earendil-works/pi-durable";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { openNodeSqliteDatabase } from "@earendil-works/pi-durable/storage/sqlite/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { githubCopilotProvider } from "@earendil-works/pi-ai/providers/github-copilot";
import type { Message as PiMessage } from "@earendil-works/pi-ai";
import type { MutableModels } from "@earendil-works/pi-ai/models";
import { FileCredentialStore } from "./credentials.ts";
import { ArchiveDoc } from "./archive.ts";
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
import { JobTask, onJobOutput, onJobStatusChange, type JobOutputListener, type JobStatusListener } from "./jobTask.ts";

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
  models: MutableModels;
  close(): Promise<void>;
};

export type OpenEngineOptions = {
  dbPath: string;
  authPath?: string;
  /** Working directory `NodeExecutionEnv` runs job commands in. Defaults to `process.cwd()`. */
  jobWorkdir?: string;
};

/**
 * Open the Pi Durable harness over the given SQLite file, registering the GitHub Copilot
 * provider and resuming scheduling. Follows the verified recipe in the F1 card exactly: no
 * `providers` option on `createModels`, explicit `setProvider`, mandatory `resume()`,
 * `modelId` (not `id`), and `{ type: "input", content }` (not `{ text }`) on submit.
 */
/** Hardcoded fallback if the env vars are unset -- the value every conversation used before this
 * card, now also the floor `getDefaultModel()` falls back to. */
const FALLBACK_MODEL: ModelRef = { provider: "github-copilot", modelId: "claude-opus-5" };

/** L5: the global default model (provider + modelId) applied to newly created conversations.
 * Read from env on every call (not cached at module load) so tests can set
 * `CPD_DEFAULT_MODEL_PROVIDER`/`CPD_DEFAULT_MODEL_ID` per case; either var absent falls back to
 * `FALLBACK_MODEL`, today's hardcoded value. No secrets -- just provider/model names. */
export function getDefaultModel(): ModelRef {
  const provider = process.env.CPD_DEFAULT_MODEL_PROVIDER;
  const modelId = process.env.CPD_DEFAULT_MODEL_ID;
  if (provider && modelId) return { provider, modelId };
  return FALLBACK_MODEL;
}

/** Shared lead-agent configuration: the model/instructions every CPD-owned conversation opens
 * with. Both the boot root (`harness.root`) and a freshly created session
 * (`harness.createConversation`, I1) apply the same config so they never diverge. */
function leadAgentConfig(): {
  model: ModelRef;
  instructions: string;
} {
  return {
    model: getDefaultModel(),
    instructions: "You are the CPD lead agent.",
  };
}

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

  const registry = createRegistry();
  registry.install({ name: "cpd", tasks: [JobTask] });

  const storage = await openNodeSqliteStorage(options.dbPath);
  const workdir = options.jobWorkdir ?? process.cwd();
  const harness = await Harness.open(
    storage,
    {
      models,
      registry,
      env: async () => new NodeExecutionEnv({ cwd: workdir }),
      onReport: (error) => console.error("[cpd-daemon] harness report:", error),
    },
    ctx,
  );
  harness.resume();

  const root = await harness.root(ctx, { agent: leadAgentConfig() });

  const titleDb = await openNodeSqliteDatabase(options.dbPath);
  const titleStore = await openTitleStore(titleDb);

  return {
    harness,
    root,
    storage,
    titleStore,
    models,
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
): Promise<{ status: "done" | "unanswered"; reason?: string; detail?: string }> {
  // L2: queue (don't race or drop) a message sent while the conversation is already busy, and
  // answer queued submissions in the order they arrived.
  const submission = await root.submit({ type: "input", content: text, whenBusy: "followUp" }, ctx);
  const settled = await submission.wait(ctx);
  if (settled.status === "unanswered") {
    // `detail` carries the real provider/model text (e.g. a content-policy refusal), while
    // `reason` is only a short code like "model_error" -- see L1.
    const detail = typeof settled.detail === "string" ? settled.detail : undefined;
    console.error("[cpd-daemon] submission unanswered:", settled.reason, detail);
    return { status: settled.status, reason: settled.reason, detail };
  }
  return { status: settled.status };
}

/** Count of this conversation's queued-but-not-yet-placed submissions (`pi.inbox`), the "N
 * queued" figure the `messages` SSE frame carries (L2). No queue -> `0`, never undefined. */
export async function getQueueDepth(engine: Engine, conversationId: ConversationId): Promise<number> {
  const state = await engine.harness.snapshot(InboxDoc, conversationId, ctx);
  return state?.items.length ?? 0;
}

/** Abort the in-flight run on `conversationId`, or root if undefined. Idempotent -- safe to
 * call when nothing is running (Conversation.abort() per pi-durable's harness/types.d.ts is
 * documented to withdraw queued inputs and abort the ordinary ownership scope either way). */
export async function abortRun(engine: Engine, conversationId: number | undefined): Promise<void> {
  const id = conversationId ?? (engine.root.id as unknown as number);
  const conversation = await getConversation(engine, id);
  if (!conversation) return;
  await conversation.abort(ctx);
}

/** Look up a conversation handle by id, falling back to the root if not found. */
export async function getConversation(
  engine: Engine,
  id: number,
): Promise<Conversation | undefined> {
  if (id === (engine.root.id as unknown as number)) return engine.root;
  return engine.harness.conversation(id as unknown as ConversationId, ctx);
}

// --- L5: per-conversation model, inherited from a global default on create --------------------

/** The provider/model catalog from the configured registry (today: GitHub Copilot only). Flat
 * `{provider, modelId}` pairs -- the real listing API (`models.getModels()`), never a hardcoded
 * list, so a future provider shows up here without a code change. */
export function getModelCatalog(engine: Engine): ModelRef[] {
  const out: ModelRef[] = [];
  for (const provider of engine.models.getProviders()) {
    for (const model of engine.models.getModels(provider.id)) {
      out.push({ provider: provider.id, modelId: model.id });
    }
  }
  return out;
}

/** Read the resolved `{provider, modelId}` of one conversation's `pi.agent`, falling back to the
 * global default (L5: inherited only at creation time, but unset `pi.agent.model` -- e.g. a
 * pre-L5 conversation -- still resolves to the current default rather than `undefined`). */
export async function getConversationModel(engine: Engine, conversationId: number): Promise<ModelRef> {
  const state = await engine.harness.snapshot(AgentDoc, conversationId as unknown as ConversationId, ctx);
  return state?.model ?? getDefaultModel();
}

/** Write `model` into one conversation's `pi.agent`, scoped to exactly that conversation --
 * never the global default and never any other conversation (D: PATCH must not leak across
 * conversations or silently rewrite the default). Throws `UnknownConversationError` for an
 * unknown conversation. */
export async function setConversationModel(engine: Engine, conversationId: number, model: ModelRef): Promise<void> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);
  await engine.harness.commit(async (tx) => {
    await configure(tx, conversationId as unknown as ConversationId, { model });
  }, ctx);
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
  const archived = new Map<number, boolean>();
  for (const item of items) {
    titles.set(item.id, await titleForConversation(engine, item.id));
    archived.set(item.id, await isArchived(engine, item.id));
  }
  return shapeConversationTree(items, (id) => titles.get(id) ?? "Untitled", (id) => archived.get(id) ?? false);
}

// --- L4: CPD-owned archived flag, backed by a pi document (cpd.archive, see archive.ts) -------

/** Whether a conversation is archived. Defaults to `false` for any conversation, including one
 * that has never had the doc committed (pi's `snapshot` returns `undefined` in that case). */
export async function isArchived(engine: Engine, conversationId: number): Promise<boolean> {
  const state = await engine.harness.snapshot(ArchiveDoc, conversationId as unknown as ConversationId, ctx);
  return state?.archived ?? false;
}

/** Set (or clear) the archived flag for a conversation. Idempotent: setting the same value twice
 * is a no-op write and never touches pi's own `entries`/`conversations` tables. */
export async function setArchived(engine: Engine, conversationId: number, archived: boolean): Promise<void> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);
  await engine.harness.commit(async (tx) => {
    const draft = await tx.doc(ArchiveDoc, conversation.id);
    draft.archived = archived;
  }, ctx);
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
        model: getDefaultModel(),
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
export type { ModelRef };

/**
 * Create a brand-new ROOT conversation (I1) -- `parentId: null`, not a fork of anything. Reuses
 * `leadAgentConfig()`, the same agent config `openEngine`'s boot root applies, per the card's
 * instruction not to duplicate creation logic. Immediately usable: `getPlan` returns `{jobs:[]}`
 * (lazy `PlanDoc`, same as any other conversation) and `/api/prompt` can submit into it right away.
 */
export async function createSession(engine: Engine, title?: string): Promise<{ id: number }> {
  const conversation = await engine.harness.createConversation(
    {
      ownership: { kind: "ownerless" },
      agent: leadAgentConfig(),
    },
    ctx,
  );
  const id = conversation.id as unknown as number;
  const resolvedTitle = title && title.trim() !== "" ? title : "New session";
  await engine.titleStore.set(id, resolvedTitle);
  return { id };
}

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

// --- J1: "blocked" means a human must act; /api/attention is the queue's data source ---------

/** In-memory "entered attention" timestamps, keyed by `${conversationId}:${jobId}`. Jobs carry
 * no timestamp fields (standing "keep contracts at minimum" instruction, D115) so this is tracked
 * out-of-band rather than widening the persisted `Job` shape for one derived view. Not durable
 * across a restart -- an item that re-enters the queue after a crash gets a fresh `at`, which is
 * an acceptable loss for an ordering hint on a queue, not a correctness-bearing field. */
const attentionTimestamps = new Map<string, number>();

/** Record (or refresh) the moment a job entered an attention-worthy status. Called from
 * `patchPlanJob` and from `JobTask`'s own status mirroring (jobTask.ts), the only two places a
 * job's status changes. */
export function touchAttention(conversationId: number, jobId: string): void {
  attentionTimestamps.set(`${conversationId}:${jobId}`, Date.now());
}

function clearAttention(conversationId: number, jobId: string): void {
  attentionTimestamps.delete(`${conversationId}:${jobId}`);
}

export type AttentionItem = {
  jobId: string;
  conversationId: number;
  conversationTitle: string;
  jobTitle: string;
  status: "blocked" | "failed";
  reason: string;
  at: number;
};

/** Every job across every conversation that needs a human: `blocked` (explicitly needs one) and
 * `failed` (a failure needs one too). Newest first. Empty DB / nothing needing attention ->
 * `[]`, never a 404 -- this is a dashboard query, not a resource lookup. */
export async function getAttentionItems(engine: Engine): Promise<AttentionItem[]> {
  const conversations = await listConversations(engine);
  const items: AttentionItem[] = [];

  for (const conversation of conversations) {
    const jobs = await getPlan(engine, conversation.id);
    for (const job of jobs) {
      if (job.status !== "blocked" && job.status !== "failed") {
        clearAttention(conversation.id, job.id);
        continue;
      }
      const key = `${conversation.id}:${job.id}`;
      let at = attentionTimestamps.get(key);
      if (at === undefined) {
        at = Date.now();
        attentionTimestamps.set(key, at);
      }
      items.push({
        jobId: job.id,
        conversationId: conversation.id,
        conversationTitle: conversation.title,
        jobTitle: job.title,
        status: job.status,
        reason: job.status === "blocked" ? (job.blockedReason ?? "") : "Job failed",
        at,
      });
    }
  }

  return items.sort((a, b) => b.at - a.at);
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
  input: { title: string; needs?: string[]; command?: string },
): Promise<Job> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);

  const current = await getPlan(engine, conversationId);
  const job: Job = {
    id: crypto.randomUUID(),
    title: input.title,
    status: "draft",
    needs: input.needs ?? [],
    command: input.command ?? null,
    taskId: null,
    blockedReason: null,
  };
  const nextJobs = addJob(current, job);
  await commitJobs(engine, conversationId, nextJobs);
  return job;
}

/** Patch an existing job's title/status/needs/blockedReason. Throws `UnknownJobIdError` for an
 * unknown id. `blockedReason` follows the same `undefined`=leave / `null`=clear semantics as
 * `command` (D122); `setStatus` already clears it when `status` moves off `blocked` (J1), but an
 * explicit `blockedReason` patch is also honoured so the reason can be set/updated independently
 * (e.g. setting status and reason in the same PATCH). */
export async function patchPlanJob(
  engine: Engine,
  conversationId: number,
  patch: {
    id: string;
    status?: JobStatus;
    title?: string;
    needs?: string[];
    command?: string | null;
    blockedReason?: string | null;
  },
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
  if (patch.command !== undefined) {
    current = current.map((job) => (job.id === patch.id ? { ...job, command: patch.command! } : job));
  }
  if (patch.blockedReason !== undefined) {
    current = current.map((job) => (job.id === patch.id ? { ...job, blockedReason: patch.blockedReason! } : job));
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

// --- H6: jobs actually run as cpd.job tasks --------------------------------------------------

export class UnknownRunJobIdError extends Error {
  readonly id: string;
  constructor(id: string) {
    super(`Unknown job id: ${id}`);
    this.id = id;
    this.name = "UnknownRunJobIdError";
  }
}

export class JobAlreadyRunningError extends Error {
  readonly id: string;
  constructor(id: string) {
    super(`Job already running: ${id}`);
    this.id = id;
    this.name = "JobAlreadyRunningError";
  }
}

export class JobDependencyCycleError extends Error {
  readonly ids: string[];
  constructor(ids: string[]) {
    super(`Cycle detected while resolving job dependencies: ${ids.join(" -> ")}`);
    this.ids = ids;
    this.name = "JobDependencyCycleError";
  }
}

/** A job (or one of its prerequisites) is `blocked` and therefore cannot run (J1). `blockingId`
 * is always the actual blocked job -- for a dependent whose prerequisite is blocked, that is the
 * prerequisite's id, named explicitly so the caller is never left guessing which job needs a
 * human. Surfaced as 409. */
export class JobBlockedError extends Error {
  readonly blockingId: string;
  constructor(blockingId: string) {
    super(`Job blocked, cannot run: ${blockingId}`);
    this.blockingId = blockingId;
    this.name = "JobBlockedError";
  }
}

/** Resolve `jobId`'s dependencies to real Durable task ids, auto-starting any dependency that
 * has not been started yet (depth-first over `needs`), recursively. `needs` holds job ids in the
 * plan document and API (the user-facing contract); Pi Durable's `on:` wants task ids, and those
 * two id spaces must never be cast between each other (H15). Returns the resolved task ids for
 * `jobId`'s direct dependencies, alongside the possibly-updated job list (dependencies that were
 * auto-started now carry a `taskId`). Throws `JobDependencyCycleError` on a cycle rather than
 * deadlocking the engine. */
async function resolveDependencyTaskIds(
  engine: Engine,
  conversationId: number,
  convId: ConversationId,
  jobs: Job[],
  jobId: string,
  visiting: string[],
): Promise<{ taskIds: TaskId[]; jobs: Job[] }> {
  if (visiting.includes(jobId)) {
    throw new JobDependencyCycleError([...visiting, jobId]);
  }

  const job = jobs.find((j) => j.id === jobId);
  if (!job) throw new UnknownRunJobIdError(jobId);

  const taskIds: TaskId[] = [];
  let currentJobs = jobs;
  const nextVisiting = [...visiting, jobId];

  for (const needId of job.needs) {
    const needJob = currentJobs.find((j) => j.id === needId);
    if (!needJob) throw new UnknownRunJobIdError(needId);

    if (needJob.status === "blocked") {
      throw new JobBlockedError(needId);
    }

    if (needJob.taskId && needJob.status !== "draft") {
      taskIds.push(needJob.taskId as unknown as TaskId);
      continue;
    }

    // Unstarted (or never-reached) dependency: auto-start it first, depth-first.
    const resolved = await resolveDependencyTaskIds(
      engine,
      conversationId,
      convId,
      currentJobs,
      needId,
      nextVisiting,
    );
    currentJobs = resolved.jobs;

    const depTaskId = await engine.harness.commit(async (tx) => {
      return tx.createTask(
        JobTask,
        { jobId: needId, conversationId: convId, command: needJob.command ?? undefined, dependsOn: resolved.taskIds },
        { ownership: { kind: "conversation" }, conversationId: convId },
      );
    }, ctx);

    const depTaskIdStr = String(depTaskId);
    currentJobs = currentJobs.map((j) => (j.id === needId ? { ...j, taskId: depTaskIdStr } : j));
    await commitJobs(engine, conversationId, currentJobs);

    taskIds.push(depTaskId);
  }

  return { taskIds, jobs: currentJobs };
}

/** Start a job's `cpd.job` task. Throws `UnknownConversationError`/`UnknownRunJobIdError` for an
 * unknown conversation/job, `JobAlreadyRunningError` if the job already has a live task (status
 * `queued` or `running`) rather than starting a second one, and `JobDependencyCycleError` if
 * resolving its dependencies finds a cycle. Unstarted dependencies are auto-started first
 * (depth-first over `needs`) so "Run" on a leaf job runs its prerequisites then itself. */
export async function runPlanJob(
  engine: Engine,
  conversationId: number,
  jobId: string,
): Promise<{ taskId: string }> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);

  let jobs = await getPlan(engine, conversationId);
  const job = jobs.find((j) => j.id === jobId);
  if (!job) throw new UnknownRunJobIdError(jobId);
  if (job.status === "blocked") {
    throw new JobBlockedError(jobId);
  }
  if (job.status === "queued" || job.status === "running") {
    throw new JobAlreadyRunningError(jobId);
  }

  const convId = conversationId as unknown as ConversationId;

  const resolved = await resolveDependencyTaskIds(engine, conversationId, convId, jobs, jobId, []);
  jobs = resolved.jobs;
  const dependencyTaskIds = resolved.taskIds;

  const taskId = await engine.harness.commit(async (tx) => {
    return tx.createTask(
      JobTask,
      { jobId, conversationId: convId, command: job.command ?? undefined, dependsOn: dependencyTaskIds },
      { ownership: { kind: "conversation" }, conversationId: convId },
    );
  }, ctx);

  const taskIdStr = String(taskId);
  await commitJobs(
    engine,
    conversationId,
    jobs.map((j) => (j.id === jobId ? { ...j, taskId: taskIdStr } : j)),
  );

  return { taskId: taskIdStr };
}

/** Abort a job's running task. Throws `UnknownConversationError`/`UnknownRunJobIdError` for an
 * unknown conversation/job. A job with no live task is a no-op. */
export async function abortPlanJob(engine: Engine, conversationId: number, jobId: string): Promise<void> {
  const conversation = await getConversation(engine, conversationId);
  if (!conversation) throw new UnknownConversationError(conversationId);

  const jobs = await getPlan(engine, conversationId);
  const job = jobs.find((j) => j.id === jobId);
  if (!job) throw new UnknownRunJobIdError(jobId);
  if (!job.taskId) return;

  await engine.harness.abortTask(job.taskId as unknown as TaskId, ctx);
}

export { onJobOutput, type JobOutputListener, onJobStatusChange, type JobStatusListener };
