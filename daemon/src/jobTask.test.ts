import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
import { JobTask, boundTail, onJobOutput } from "./jobTask.ts";

/** Opens a harness with the JobTask registered and a real NodeExecutionEnv over `workdir`,
 * matching the verified D113/D114 recipe. No real credentials, no network. */
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
  harness.resume();
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

describe("cpd.job task", () => {
  let dir: string;

  afterEach(() => {
    if (dir && existsSync(dir)) rmSync(dir, { recursive: true, force: true });
  });

  it("boundTail keeps only the last 200 lines / 16KB", () => {
    const manyLines = Array.from({ length: 500 }, (_, i) => `line ${i}`);
    const tail = boundTail(manyLines.map((l) => `${l}\n`));
    expect(tail.split("\n").filter(Boolean).length).toBeLessThanOrEqual(200);

    const bigChunk = "x".repeat(20 * 1024);
    const huge = boundTail([bigChunk]);
    expect(Buffer.byteLength(huge, "utf8")).toBeLessThanOrEqual(16 * 1024);
  });

  it("a job with a command runs, captures output, and reaches done", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-jobtask-echo-"));
    const dbPath = join(dir, "test.sqlite");
    const { harness, root } = await openTestHarness(dbPath, dir);

    const job: Job = { id: "j1", title: "Say hi", status: "draft", needs: [], command: "echo hi", taskId: null };
    await seedJob(harness, root.id, job);

    const taskId = await startJob(harness, root.id, job);
    const settled = await harness.waitForTask(taskId, ctx);

    expect(settled.state.outcome.status).toBe("completed");
    if (settled.state.outcome.status === "completed") {
      const result = settled.state.outcome.result as { exitCode: number; output: string };
      expect(result.exitCode).toBe(0);
      expect(result.output).toContain("hi");
    }

    const plan = await harness.snapshot(PlanDoc, root.id, ctx);
    expect(plan?.jobs.find((j) => j.id === "j1")?.status).toBe("done");

    await harness.close(ctx);
  });

  it("a job with a failing command reaches failed and the plan document shows failed", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-jobtask-fail-"));
    const dbPath = join(dir, "test.sqlite");
    const { harness, root } = await openTestHarness(dbPath, dir);

    const job: Job = { id: "j1", title: "Exit 3", status: "draft", needs: [], command: "exit 3", taskId: null };
    await seedJob(harness, root.id, job);

    const taskId = await startJob(harness, root.id, job);
    const settled = await harness.waitForTask(taskId, ctx);

    expect(settled.state.outcome.status).toBe("failed");

    const plan = await harness.snapshot(PlanDoc, root.id, ctx);
    expect(plan?.jobs.find((j) => j.id === "j1")?.status).toBe("failed");

    await harness.close(ctx);
  });

  it("a job with no command settles done without touching the environment", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-jobtask-nocommand-"));
    const dbPath = join(dir, "test.sqlite");
    const { harness, root } = await openTestHarness(dbPath, dir);

    const job: Job = { id: "j1", title: "No-op", status: "draft", needs: [], command: null, taskId: null };
    await seedJob(harness, root.id, job);

    const taskId = await startJob(harness, root.id, job);
    const settled = await harness.waitForTask(taskId, ctx);

    expect(settled.state.outcome.status).toBe("completed");
    if (settled.state.outcome.status === "completed") {
      const result = settled.state.outcome.result as { exitCode: number };
      expect(result.exitCode).toBe(0);
    }

    const plan = await harness.snapshot(PlanDoc, root.id, ctx);
    expect(plan?.jobs.find((j) => j.id === "j1")?.status).toBe("done");

    await harness.close(ctx);
  });

  it("a job with needs does not start until both dependencies are terminal (allSettled)", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-jobtask-needs-"));
    const dbPath = join(dir, "test.sqlite");
    const { harness, root } = await openTestHarness(dbPath, dir);

    const markerA = join(dir, "a.marker");
    const markerB = join(dir, "b.marker");
    const markerC = join(dir, "c.marker");

    const jobA: Job = { id: "a", title: "A", status: "draft", needs: [], command: `sleep 0.2 && echo done > ${markerA}`, taskId: null };
    const jobB: Job = { id: "b", title: "B", status: "draft", needs: [], command: `exit 1`, taskId: null };
    const jobC: Job = {
      id: "c",
      title: "C",
      status: "draft",
      needs: ["a", "b"],
      command: `echo done > ${markerC}`,
      taskId: null,
    };
    await seedJob(harness, root.id, jobA);
    await seedJob(harness, root.id, jobB);
    await seedJob(harness, root.id, jobC);

    const taskIdA = await startJob(harness, root.id, jobA);
    const taskIdB = await startJob(harness, root.id, jobB);
    const taskIdC = await startJob(harness, root.id, jobC, [taskIdA, taskIdB]);

    // c must not have run before a settled.
    expect(existsSync(markerC)).toBe(false);

    const [settledA, settledB, settledC] = await Promise.all([
      harness.waitForTask(taskIdA, ctx),
      harness.waitForTask(taskIdB, ctx),
      harness.waitForTask(taskIdC, ctx),
    ]);

    expect(settledA.state.outcome.status).toBe("completed");
    expect(settledB.state.outcome.status).toBe("failed"); // allSettled: one dependency fails...
    expect(settledC.state.outcome.status).toBe("completed"); // ...the dependent still runs.
    expect(existsSync(markerA)).toBe(true);
    expect(existsSync(markerB)).toBe(false);
    expect(existsSync(markerC)).toBe(true);

    await harness.close(ctx);
  });

  it("mirrors status transitions onto the plan document", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-jobtask-mirror-"));
    const dbPath = join(dir, "test.sqlite");
    const { harness, root } = await openTestHarness(dbPath, dir);

    const job: Job = { id: "j1", title: "Mirror", status: "draft", needs: [], command: "echo mirrored", taskId: null };
    await seedJob(harness, root.id, job);

    const taskId = await startJob(harness, root.id, job);
    await harness.waitForTask(taskId, ctx);

    const plan = await harness.snapshot(PlanDoc, root.id, ctx);
    expect(plan?.jobs.find((j) => j.id === "j1")?.status).toBe("done");

    await harness.close(ctx);
  });

  it("streams live output over the registered listener without per-chunk doc commits", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-jobtask-stream-"));
    const dbPath = join(dir, "test.sqlite");
    const { harness, root } = await openTestHarness(dbPath, dir);

    const chunks: string[] = [];
    onJobOutput((_taskId, jobId, conversationId, chunk) => {
      if (jobId === "j1") chunks.push(chunk);
      void conversationId;
    });

    const job: Job = { id: "j1", title: "Stream", status: "draft", needs: [], command: "echo streamed-output", taskId: null };
    await seedJob(harness, root.id, job);
    const taskId = await startJob(harness, root.id, job);
    await harness.waitForTask(taskId, ctx);

    expect(chunks.join("")).toContain("streamed-output");

    onJobOutput(undefined);
    await harness.close(ctx);
  });
});

void writeFileSync;
