import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Models } from "@earendil-works/pi-ai";
import { createRegistry, Harness, type Extension } from "@earendil-works/pi-durable";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { CodingTools } from "@earendil-works/pi-durable/tools";
import type { Conversation as HostConversation } from "../durable-host/run.ts";
import type { Input } from "../durable-host/deliver.ts";
import type { Write } from "../durable-host/turn.ts";

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
  return {
    submit: async (input) => {
      const submission = await root.submit({ type: "input", content: input.content, requestId: input.requestId, whenBusy: input.whenBusy }, BACKGROUND_CONTEXT);
      return { wait: async () => (await submission.wait(BACKGROUND_CONTEXT)).status };
    },
    write: async (_write: Write) => {
      throw new Error("not implemented");
    },
    install: (extension) => registry.install(extension),
    entries: async () => (await root.viewState(BACKGROUND_CONTEXT)).value.entries as never,
    closed: harness.closed,
  };
}

/** Stub (p1-4b): the Conversation of a root and its registry; not implemented yet. */
export function conversationOf(_root: unknown, _registry: unknown, _closed: Promise<unknown>): HostConversation {
  throw new Error("not implemented");
}
