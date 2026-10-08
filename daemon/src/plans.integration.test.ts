import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach } from "vitest";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { Harness, createRegistry, type Conversation } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { fauxProvider } from "@earendil-works/pi-ai/providers/faux";
import { PlanDoc, type Job } from "./plans.ts";

/**
 * Opens a harness over `dbPath` with a faux model provider -- no real credentials, no network --
 * matching the probe pattern from D112/D113/D114.
 */
async function openTestHarness(dbPath: string): Promise<{ harness: Harness; root: Conversation }> {
  const storage = await openNodeSqliteStorage(dbPath);
  const models = createModels();
  models.setProvider(fauxProvider().provider);
  const harness = await Harness.open(
    storage,
    {
      models,
      registry: createRegistry(),
    },
    ctx,
  );
  harness.resume();
  const root = await harness.root(ctx, {
    agent: { model: { provider: "faux", modelId: "faux-1" }, instructions: "test" },
  });
  return { harness, root };
}

describe("cpd.plan document", () => {
  let dir: string;

  afterEach(() => {
    if (dir && existsSync(dir)) rmSync(dir, { recursive: true, force: true });
  });

  it("persists jobs with needs edges across a cold restart", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-plan-test-"));
    const dbPath = join(dir, "test.sqlite");

    const first = await openTestHarness(dbPath);
    const conversationId = first.root.id;

    await first.harness.commit(async (tx) => {
      const draft = await tx.doc(PlanDoc, conversationId);
      const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
      const j2: Job = { id: "j2", title: "Test", status: "draft", needs: ["j1"] };
      draft.jobs.push(j1, j2);
    }, ctx);

    const readBack = await first.harness.snapshot(PlanDoc, conversationId, ctx);
    expect(readBack?.jobs).toEqual([
      { id: "j1", title: "Build", status: "draft", needs: [] },
      { id: "j2", title: "Test", status: "draft", needs: ["j1"] },
    ]);

    await first.harness.close(ctx);

    // Cold restart: new storage handle, new Harness, same file.
    const second = await openTestHarness(dbPath);
    const reread = await second.harness.snapshot(PlanDoc, conversationId, ctx);
    expect(reread?.jobs).toEqual([
      { id: "j1", title: "Build", status: "draft", needs: [] },
      { id: "j2", title: "Test", status: "draft", needs: ["j1"] },
    ]);

    await second.harness.close(ctx);
  });

  it("a fork inherits jobs and diverges copy-on-write", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-plan-fork-test-"));
    const dbPath = join(dir, "test.sqlite");

    const { harness, root } = await openTestHarness(dbPath);

    await harness.commit(async (tx) => {
      const draft = await tx.doc(PlanDoc, root.id);
      const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
      const j2: Job = { id: "j2", title: "Test", status: "draft", needs: ["j1"] };
      draft.jobs.push(j1, j2);
    }, ctx);

    const firstEntryId = await harness.commit(async (tx) => {
      const entry = await tx.appendEntry(root.id, { kind: "test.marker" });
      return entry.id;
    }, ctx);

    const child = await root.fork(firstEntryId, { ownership: { kind: "ownerless" } }, ctx);

    const childAtCreation = await harness.snapshot(PlanDoc, child.id, ctx);
    expect(childAtCreation?.jobs).toEqual([
      { id: "j1", title: "Build", status: "draft", needs: [] },
      { id: "j2", title: "Test", status: "draft", needs: ["j1"] },
    ]);

    await harness.commit(async (tx) => {
      const draft = await tx.doc(PlanDoc, child.id);
      const j3: Job = { id: "j3", title: "Deploy", status: "draft", needs: ["j2"] };
      draft.jobs.push(j3);
    }, ctx);

    const parentAfterChildWrite = await harness.snapshot(PlanDoc, root.id, ctx);
    const childAfterChildWrite = await harness.snapshot(PlanDoc, child.id, ctx);

    expect(parentAfterChildWrite?.jobs).toEqual([
      { id: "j1", title: "Build", status: "draft", needs: [] },
      { id: "j2", title: "Test", status: "draft", needs: ["j1"] },
    ]);
    expect(childAfterChildWrite?.jobs).toEqual([
      { id: "j1", title: "Build", status: "draft", needs: [] },
      { id: "j2", title: "Test", status: "draft", needs: ["j1"] },
      { id: "j3", title: "Deploy", status: "draft", needs: ["j2"] },
    ]);

    await harness.close(ctx);
  });
});
