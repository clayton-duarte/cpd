import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach } from "vitest";
import { BACKGROUND_CONTEXT as ctx } from "@earendil-works/chord/context";
import { Harness, createRegistry, type Conversation, type TaskId } from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { NodeExecutionEnv } from "@earendil-works/pi-durable/env/node";
import { createModels } from "@earendil-works/pi-ai/models";
import { fauxProvider } from "@earendil-works/pi-ai/providers/faux";
import { PlanDoc, addJob, type Job } from "./plans.ts";
import { JobTask } from "./jobTask.ts";

/** Matches the verified recipe from plans.integration.test.ts / jobTask.test.ts: a harness with
 * the JobTask registered and a real NodeExecutionEnv over `workdir`. */
async function openTestHarness(
  dbPath: string,
  workdir: string,
): Promise<{ harness: Harness; root: Conversation }> {
  const storage = await openNodeSqliteStorage(dbPath);
  const models = createModels();
  models.setProvider(fauxProvider().provider);
  const registry = createRegistry();
  registry.install({ name: "cpd", tasks: [JobTask] });
  const harness = await Harness.open(
    storage,
    {
      models,
      registry,
      env: async () => new NodeExecutionEnv({ cwd: workdir }),
    },
    ctx,
  );
  const root = await harness.root(ctx, {
    agent: { model: { provider: "faux", modelId: "faux-1" }, instructions: "test" },
  });
  return { harness, root };
}

async function seedJob(harness: Harness, conversationId: Conversation["id"], job: Job): Promise<void> {
  await harness.commit(async (tx) => {
    const draft = await tx.doc(PlanDoc, conversationId);
    draft.jobs = addJob(draft.jobs, job);
  }, ctx);
}

async function startJob(
  harness: Harness,
  conversationId: Conversation["id"],
  job: Job,
  dependsOn: readonly TaskId[] = [],
): Promise<TaskId> {
  return harness.commit(async (tx) => {
    return tx.createTask(
      JobTask,
      { jobId: job.id, conversationId, command: job.command ?? undefined, dependsOn },
      { ownership: { kind: "conversation" }, conversationId },
    );
  }, ctx);
}

describe("crash resumption", () => {
  let dir: string;

  afterEach(() => {
    if (dir && existsSync(dir)) rmSync(dir, { recursive: true, force: true });
  });

  it("a job survives losing its harness mid-run and still reaches done", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-durability-test-"));
    const dbPath = join(dir, "test.sqlite");
    const marker = join(dir, "survived.marker");

    const first = await openTestHarness(dbPath, dir);
    first.harness.resume();

    const job: Job = {
      id: "j1",
      title: "Survive a crash",
      status: "draft",
      needs: [],
      command: `sleep 1 && echo SURVIVED >> ${marker}`,
      taskId: null,
      blockedReason: null,
    };
    await seedJob(first.harness, first.root.id, job);
    const taskId = await startJob(first.harness, first.root.id, job);

    // Give the command a moment to actually start running before we yank the harness away --
    // this must be a genuine crash-mid-flight, not "the job never began".
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(existsSync(marker)).toBe(false);

    // Simulate the crash: drop the harness and its storage handle without ever resolving the
    // task. No process.exit -- just stop driving it, the way a kill -9 would.
    await first.harness.close(ctx);

    // Cold restart: new storage handle over the same sqlite file, new Harness.
    const second = await openTestHarness(dbPath, dir);
    second.harness.resume();

    const settled = await second.harness.waitForTask(taskId, ctx);
    expect(settled.state.outcome.status).toBe("completed");

    expect(existsSync(marker)).toBe(true);
    expect(readFileSync(marker, "utf8")).toContain("SURVIVED");

    const plan = await second.harness.snapshot(PlanDoc, second.root.id, ctx);
    expect(plan?.jobs.find((j) => j.id === "j1")?.status).toBe("done");

    await second.harness.close(ctx);
  });
});
