import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Models } from "@earendil-works/pi-ai";
import { createRegistry, Harness, watchEvents, type Extension } from "@earendil-works/pi-durable";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { CodingTools } from "@earendil-works/pi-durable/tools";
import type { Conversation as HostConversation } from "../durable-host/run.ts";
import type { Input } from "../durable-host/deliver.ts";
import type { Write } from "../durable-host/turn.ts";

type Root = {
  readonly id: unknown;
  abort(ctx: typeof BACKGROUND_CONTEXT, opts: { background: boolean }): Promise<void>;
  configure(change: unknown, ctx: typeof BACKGROUND_CONTEXT): Promise<void>;
  submit(input: unknown, ctx: typeof BACKGROUND_CONTEXT): Promise<{ readonly id: number; status(ctx: typeof BACKGROUND_CONTEXT): Promise<{ status: string }>; wait(ctx: typeof BACKGROUND_CONTEXT): Promise<{ status: "done" | "unanswered" | "failed" }>; }>;
  viewState(ctx: typeof BACKGROUND_CONTEXT): Promise<{ value: { entries: unknown } }>;
  compact(instructions: string | undefined, ctx: typeof BACKGROUND_CONTEXT): Promise<number>;
  configure(change: unknown, ctx: typeof BACKGROUND_CONTEXT): Promise<void>;
  agent(ctx: typeof BACKGROUND_CONTEXT): Promise<{ model?: { provider: string; modelId: string }; thinkingLevel: string }>;
  entries(query: object, limit: number, cursor: unknown, ctx: typeof BACKGROUND_CONTEXT): Promise<{ items: readonly unknown[]; next?: object }>;
};

/** The Conversation of a root and its registry (contract §6.13): the pure part of `openConversation`, testable with no pi-durable. */
export type EventStream = {
  readonly snapshot: unknown;
  start(listener: (events: readonly unknown[]) => Promise<void>): void;
  stop(): Promise<unknown>;
  readonly closed: Promise<unknown>;
};
export type AbortOutcome = "aborted" | "already_placed" | "settled" | "not_found";
/** What `conversationOf` calls on pi-durable beyond the root: the Harness and `watchEvents`, injected so a test passes fakes (contract §6.14 "The event stream"). */
export type Durable = {
  harness: { abortSubmission(id: number, ctx: typeof BACKGROUND_CONTEXT, conversationId?: unknown): Promise<AbortOutcome> };
  watchEvents(harness: unknown, conversationId: unknown, ctx: typeof BACKGROUND_CONTEXT): Promise<{ snapshot: unknown; start(listener: (events: readonly unknown[], context: unknown) => Promise<void>): void; stop(): Promise<unknown>; closed: Promise<unknown> }>;
};
export type SubmissionOf = { id: number; status(): Promise<string>; wait(): Promise<"done" | "unanswered"> };
export type Extras = {
  submit(input: Input): Promise<SubmissionOf>;
  watch(): Promise<EventStream>;
  abort(opts: { background: boolean }): Promise<void>;
  abortSubmission(id: number): Promise<AbortOutcome>;
};

export function conversationOf(root: unknown, registry: { install(extension: Extension): void }, closed: Promise<unknown>, durable: Durable): Omit<HostConversation, "submit"> & Extras {
  const r = root as Root;
  return {
    watch: async () => {
      const stream = await durable.watchEvents(durable.harness, r.id, BACKGROUND_CONTEXT);
      return {
        snapshot: stream.snapshot,
        start: (listener: (events: readonly unknown[]) => Promise<void>) => stream.start((events) => listener(events)),
        stop: () => stream.stop(),
        closed: stream.closed,
      };
    },
    abort: (opts: { background: boolean }) => r.abort(BACKGROUND_CONTEXT, opts),
    abortSubmission: (id: number) => durable.harness.abortSubmission(id, BACKGROUND_CONTEXT, r.id),
    submit: async (input: Input) => {
      const submission = await r.submit(
        { type: "input", content: input.content, requestId: input.requestId, whenBusy: input.whenBusy, ...(input.entry === undefined ? {} : { entry: input.entry }) },
        BACKGROUND_CONTEXT,
      );
      return {
        id: submission.id,
        status: async (): Promise<string> => (await submission.status(BACKGROUND_CONTEXT)).status,
        wait: async () => (await submission.wait(BACKGROUND_CONTEXT)).status as "done" | "unanswered",
      };
    },
    write: async (write: Write) => {
      const submission = await r.submit(write, BACKGROUND_CONTEXT);
      const { status } = await submission.wait(BACKGROUND_CONTEXT);
      if (status !== "done") throw new Error(`the write ended ${status}`);
    },
    install: (extension) => registry.install(extension),
    entries: async () => (await r.viewState(BACKGROUND_CONTEXT)).value.entries as never,
    compact: (instructions) => r.compact(instructions, BACKGROUND_CONTEXT),
    configure: (change) => r.configure(change, BACKGROUND_CONTEXT),
    agent: () => r.agent(BACKGROUND_CONTEXT),
    pageEntries: async (limit, cursor) => {
      const page = await r.entries({ order: "descending" }, limit, cursor, BACKGROUND_CONTEXT);
      return { entries: page.items, ...(page.next === undefined ? {} : { next: page.next }) };
    },
    closed,
  };
}

/** The one adapter from a real pi-durable Harness to `Deps.open`. It never closes the Harness (contract §6.12). */
export async function openConversation(
  models: Models,
  o: { file: string; cwd: string; model: { provider: string; modelId: string; thinkingLevel?: string }; extension: Extension },
): Promise<Omit<HostConversation, "submit" | "entries"> & { entries(): Promise<readonly never[]>; submit(input: Omit<Input, "entry" | "reply"> & Partial<Pick<Input, "entry" | "reply">>): ReturnType<HostConversation["submit"]> }> {
  await mkdir(dirname(o.file), { recursive: true });
  const registry = createRegistry();
  registry.install(CodingTools);
  registry.install(o.extension);
  const harness = await Harness.open(
    await openNodeSqliteStorage(o.file),
    { models, registry, env: ({ cwd }) => new NodeExecutionEnv({ cwd: cwd ?? process.cwd(), shellEnv: process.env }) },
    BACKGROUND_CONTEXT,
  );
  harness.resume();
  const root = await harness.root(BACKGROUND_CONTEXT, {
    agent: { model: { provider: o.model.provider, modelId: o.model.modelId }, thinkingLevel: o.model.thinkingLevel as never, cwd: o.cwd } as never,
  });
  return conversationOf(root, registry, harness.closed, { harness, watchEvents } as never) as never;
}
