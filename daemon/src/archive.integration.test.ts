import { describe, expect, it } from "vitest";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { Harness, createRegistry, type Conversation } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { fauxProvider } from "@earendil-works/pi-ai/providers/faux";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ArchiveDoc } from "./archive.ts";

async function openTestHarness(dbPath: string): Promise<{ harness: Harness; root: Conversation }> {
  const storage = await openNodeSqliteStorage(dbPath);
  const models = createModels();
  models.setProvider(fauxProvider().provider);
  const harness = await Harness.open(storage, { models, registry: createRegistry() }, ctx);
  harness.resume();
  const root = await harness.root(ctx, {
    agent: { model: { provider: "faux", modelId: "faux-1" }, instructions: "test" },
  });
  return { harness, root };
}

describe("cpd.archive document", () => {
  it("defaults to unarchived for a brand-new conversation", async () => {
    const dir = mkdtempSync(join(tmpdir(), "cpd-archive-test-"));
    const dbPath = join(dir, "test.sqlite");
    try {
      const { harness, root } = await openTestHarness(dbPath);
      // Touch the doc once so pi-durable materializes its initial state -- `snapshot` on a
      // conversation that has never had this doc committed returns undefined (matches the
      // `cpd.plan` precedent in plans.integration.test.ts, which always snapshots after a commit).
      await harness.commit(async (tx) => {
        await tx.doc(ArchiveDoc, root.id);
      }, ctx);
      const state = await harness.snapshot(ArchiveDoc, root.id, ctx);
      expect(state?.archived).toBe(false);
      await harness.close(ctx);
    } finally {
      if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
    }
  });

  it("persists an archived flag across a cold restart", async () => {
    const dir = mkdtempSync(join(tmpdir(), "cpd-archive-restart-test-"));
    const dbPath = join(dir, "test.sqlite");
    try {
      const first = await openTestHarness(dbPath);
      await first.harness.commit(async (tx) => {
        const draft = await tx.doc(ArchiveDoc, first.root.id);
        draft.archived = true;
      }, ctx);
      await first.harness.close(ctx);

      const second = await openTestHarness(dbPath);
      const state = await second.harness.snapshot(ArchiveDoc, second.root.id, ctx);
      expect(state?.archived).toBe(true);
      await second.harness.close(ctx);
    } finally {
      if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
    }
  });
});
