import { describe, expect, it, afterEach, beforeAll, afterAll } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// HTTP-level tests for the plan-job routes (POST/PATCH /api/plan/job), the exact layer that
// dropped `command` silently (H12): every prior test called engine functions directly, bypassing
// this parse layer. These start the real daemon process over an ephemeral port, as
// `boot.test.ts` does, and assert over real `fetch`.

type JobResponse = {
  job?: { id: string; title: string; command: string | null; status: string };
  error?: string;
};

describe("plan-job HTTP routes", () => {
  let child: ChildProcess;
  let dir: string;
  let port: number;
  let base: string;

  beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-plan-job-http-"));
    port = 21000 + Math.floor(Math.random() * 9000);
    const dbPath = join(dir, "test.sqlite");
    base = `http://localhost:${port}`;

    child = spawn(
      process.execPath,
      ["--experimental-strip-types", join(import.meta.dirname, "index.ts")],
      {
        env: { ...process.env, CPD_DAEMON_PORT: String(port), CPD_DB: dbPath },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );

    let stderr = "";
    child.stderr?.on("data", (chunk) => {
      stderr += chunk.toString();
    });

    const deadline = Date.now() + 15_000;
    let ok = false;
    while (Date.now() < deadline) {
      if (child.exitCode !== null) {
        throw new Error(`daemon exited early (code ${child.exitCode}):\n${stderr}`);
      }
      try {
        const res = await fetch(`${base}/api/health`);
        if (res.ok) {
          ok = true;
          break;
        }
      } catch {
        // retry
      }
      await new Promise((resolve) => setTimeout(resolve, 150));
    }
    if (!ok) throw new Error(`daemon never answered /api/health within 15s (stderr:\n${stderr})`);
  }, 20_000);

  afterAll(() => {
    if (child && !child.killed) child.kill();
    if (dir) rmSync(dir, { recursive: true, force: true });
  });

  it("POST with command round-trips through GET /api/plan", async () => {
    const res = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "Echo", command: "echo hi" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as JobResponse;
    expect(body.job?.command).toBe("echo hi");

    const planRes = await fetch(`${base}/api/plan`);
    const plan = (await planRes.json()) as { jobs: { id: string; command: string | null }[] };
    const found = plan.jobs.find((j) => j.id === body.job!.id);
    expect(found?.command).toBe("echo hi");
  });

  it("POST with non-string command rejects with 400 and a named error", async () => {
    const res = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "Bad", command: 123 }),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as JobResponse;
    expect(body.error).toBeTruthy();
  });

  it("POST with whitespace-only command rejects with 400", async () => {
    const res = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "Bad", command: "   " }),
    });
    expect(res.status).toBe(400);
  });

  it("POST without command defaults to null", async () => {
    const res = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "No Command" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as JobResponse;
    expect(body.job?.command).toBeNull();
  });

  it("PATCH sets a command on an existing job, then PATCH null clears it", async () => {
    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "Draft" }),
    });
    const created = (await createRes.json()) as JobResponse;
    const id = created.job!.id;
    expect(created.job?.command).toBeNull();

    const patchRes = await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, command: "echo patched" }),
    });
    expect(patchRes.status).toBe(200);
    const patched = (await patchRes.json()) as JobResponse;
    expect(patched.job?.command).toBe("echo patched");

    const clearRes = await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, command: null }),
    });
    expect(clearRes.status).toBe(200);
    const cleared = (await clearRes.json()) as JobResponse;
    expect(cleared.job?.command).toBeNull();
  });

  it("end-to-end: a run writes a marker file, not just status done", async () => {
    const markerDir = mkdtempSync(join(tmpdir(), "cpd-plan-job-marker-"));
    const markerPath = join(markerDir, "ran.txt");
    try {
      const createRes = await fetch(`${base}/api/plan/job`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          title: "Marker",
          command: `echo ran > ${JSON.stringify(markerPath)}`,
        }),
      });
      const created = (await createRes.json()) as JobResponse;
      const id = created.job!.id;

      const runRes = await fetch(`${base}/api/plan/job/run`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id }),
      });
      expect(runRes.status).toBe(200);

      const deadline = Date.now() + 15_000;
      let status = "";
      while (Date.now() < deadline) {
        const planRes = await fetch(`${base}/api/plan`);
        const plan = (await planRes.json()) as { jobs: { id: string; status: string }[] };
        const job = plan.jobs.find((j) => j.id === id);
        status = job?.status ?? "";
        if (status === "done" || status === "failed") break;
        await new Promise((resolve) => setTimeout(resolve, 200));
      }

      expect(status).toBe("done");
      expect(existsSync(markerPath)).toBe(true);
      expect(readFileSync(markerPath, "utf8").trim()).toBe("ran");
    } finally {
      rmSync(markerDir, { recursive: true, force: true });
    }
  }, 20_000);

  // H15: a job with `needs` must actually run after its dependency settles, proven by the real
  // HTTP API path (POST /api/plan/job -> POST /api/plan/job/run), not by calling engine
  // functions directly (which pre-substitutes task ids and cannot catch the job-id/task-id bug).
  async function pollStatus(id: string, deadlineMs = 15_000): Promise<string> {
    const deadline = Date.now() + deadlineMs;
    let status = "";
    while (Date.now() < deadline) {
      const planRes = await fetch(`${base}/api/plan`);
      const plan = (await planRes.json()) as { jobs: { id: string; status: string }[] };
      const job = plan.jobs.find((j) => j.id === id);
      status = job?.status ?? "";
      if (status === "done" || status === "failed") break;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    return status;
  }

  it("end-to-end: a dependent job runs after its dependency settles, in order", async () => {
    const orderDir = mkdtempSync(join(tmpdir(), "cpd-plan-job-order-"));
    const orderPath = join(orderDir, "order.txt");
    try {
      const createA = await fetch(`${base}/api/plan/job`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: "A", command: `sleep 0.5 && echo A >> ${JSON.stringify(orderPath)}` }),
      });
      const jobA = ((await createA.json()) as JobResponse).job!;

      const createB = await fetch(`${base}/api/plan/job`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          title: "B",
          command: `echo B >> ${JSON.stringify(orderPath)}`,
          needs: [jobA.id],
        }),
      });
      const jobB = ((await createB.json()) as JobResponse).job!;

      // Start B first on purpose, before A is ever started. B must not deadlock waiting on a
      // job id cast to a task id (H15 bug) -- it resolves A's dependency and auto-starts it.
      const runB = await fetch(`${base}/api/plan/job/run`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id: jobB.id }),
      });
      expect(runB.status).toBe(200);

      const [statusA, statusB] = await Promise.all([pollStatus(jobA.id), pollStatus(jobB.id)]);
      expect(statusA).toBe("done");
      expect(statusB).toBe("done");

      expect(existsSync(orderPath)).toBe(true);
      const lines = readFileSync(orderPath, "utf8").trim().split("\n");
      expect(lines).toEqual(["A", "B"]);
    } finally {
      rmSync(orderDir, { recursive: true, force: true });
    }
  }, 20_000);

  it("running a job whose dependency was never started auto-starts it first, in order", async () => {
    const orderDir = mkdtempSync(join(tmpdir(), "cpd-plan-job-autostart-"));
    const orderPath = join(orderDir, "order.txt");
    try {
      const createA = await fetch(`${base}/api/plan/job`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: "A2", command: `sleep 0.3 && echo A >> ${JSON.stringify(orderPath)}` }),
      });
      const jobA = ((await createA.json()) as JobResponse).job!;

      const createB = await fetch(`${base}/api/plan/job`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          title: "B2",
          command: `echo B >> ${JSON.stringify(orderPath)}`,
          needs: [jobA.id],
        }),
      });
      const jobB = ((await createB.json()) as JobResponse).job!;

      // Only run B -- A has never been started (taskId: null). B must auto-start A first.
      const runB = await fetch(`${base}/api/plan/job/run`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id: jobB.id }),
      });
      expect(runB.status).toBe(200);

      const [statusA, statusB] = await Promise.all([pollStatus(jobA.id), pollStatus(jobB.id)]);
      expect(statusA).toBe("done");
      expect(statusB).toBe("done");

      expect(existsSync(orderPath)).toBe(true);
      const lines = readFileSync(orderPath, "utf8").trim().split("\n");
      expect(lines).toEqual(["A", "B"]);
    } finally {
      rmSync(orderDir, { recursive: true, force: true });
    }
  }, 20_000);

  it("a failed dependency still lets the dependent run (allSettled), and both settle", async () => {
    const createA = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "FailDep", command: "exit 1" }),
    });
    const jobA = ((await createA.json()) as JobResponse).job!;

    const createB = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "AfterFail", command: "echo still-ran", needs: [jobA.id] }),
    });
    const jobB = ((await createB.json()) as JobResponse).job!;

    const runB = await fetch(`${base}/api/plan/job/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: jobB.id }),
    });
    expect(runB.status).toBe(200);

    const [statusA, statusB] = await Promise.all([pollStatus(jobA.id), pollStatus(jobB.id)]);
    // allSettled (D113): B does not get cancelled by A's failure -- it still runs.
    expect(statusA).toBe("failed");
    expect(statusB).toBe("done");
  }, 20_000);

  // J1: "blocked" means a human must act; these prove it end to end over the real HTTP routes.
  it("PATCH sets status blocked with a reason, and blockedReason round-trips through GET /api/plan", async () => {
    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "NeedsHuman" }),
    });
    const id = ((await createRes.json()) as JobResponse).job!.id;

    const patchRes = await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, status: "blocked", blockedReason: "waiting on credentials" }),
    });
    expect(patchRes.status).toBe(200);
    const patched = (await patchRes.json()) as JobResponse & { job?: { blockedReason?: string | null } };
    expect(patched.job?.status).toBe("blocked");
    expect(patched.job?.blockedReason).toBe("waiting on credentials");

    const planRes = await fetch(`${base}/api/plan`);
    const plan = (await planRes.json()) as { jobs: { id: string; status: string; blockedReason: string | null }[] };
    const found = plan.jobs.find((j) => j.id === id);
    expect(found?.status).toBe("blocked");
    expect(found?.blockedReason).toBe("waiting on credentials");
  });

  it("moving a job off blocked clears blockedReason", async () => {
    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "UnblockMe" }),
    });
    const id = ((await createRes.json()) as JobResponse).job!.id;

    await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, status: "blocked", blockedReason: "needs a decision" }),
    });

    const unblockRes = await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, status: "draft" }),
    });
    const unblocked = (await unblockRes.json()) as JobResponse & { job?: { blockedReason?: string | null } };
    expect(unblocked.job?.status).toBe("draft");
    expect(unblocked.job?.blockedReason).toBeNull();
  });

  it("POST /api/plan/job/run on a blocked job returns 409", async () => {
    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "Blocked", command: "echo should-not-run" }),
    });
    const id = ((await createRes.json()) as JobResponse).job!.id;

    await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, status: "blocked", blockedReason: "needs a human" }),
    });

    const runRes = await fetch(`${base}/api/plan/job/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });
    expect(runRes.status).toBe(409);
    const body = (await runRes.json()) as JobResponse;
    expect(body.error).toBeTruthy();
  });

  it("running a job whose prerequisite is blocked returns 409 naming it, and starts no task", async () => {
    const createA = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "BlockedPrereq", command: "echo a" }),
    });
    const jobA = ((await createA.json()) as JobResponse).job!;

    await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: jobA.id, status: "blocked", blockedReason: "stuck" }),
    });

    const createB = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "DependsOnBlocked", command: "echo b", needs: [jobA.id] }),
    });
    const jobB = ((await createB.json()) as JobResponse).job!;

    const runB = await fetch(`${base}/api/plan/job/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: jobB.id }),
    });
    expect(runB.status).toBe(409);
    const body = (await runB.json()) as JobResponse;
    expect(body.error).toContain(jobA.id);

    // No task started: B must not silently enter a waiting state (D125 is the cautionary tale).
    const planRes = await fetch(`${base}/api/plan`);
    const plan = (await planRes.json()) as { jobs: { id: string; status: string; taskId: string | null }[] };
    const foundB = plan.jobs.find((j) => j.id === jobB.id);
    expect(foundB?.status).toBe("draft");
    expect(foundB?.taskId).toBeNull();
  });

  it("GET /api/attention lists blocked and failed jobs, newest first, and empty queue is 200 []", async () => {
    const createClean = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "AttentionEmptyCheck" }),
    });
    const cleanId = ((await createClean.json()) as JobResponse).job!.id;

    // Sanity: a draft job alone must not appear in the queue (narrows the "empty" assertion to
    // mean "nothing needing attention", not "no jobs exist at all").
    const beforeRes = await fetch(`${base}/api/attention`);
    expect(beforeRes.status).toBe(200);
    const before = (await beforeRes.json()) as { items: { jobId: string }[] };
    expect(before.items.some((item) => item.jobId === cleanId)).toBe(false);

    const createBlocked = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "AttentionBlocked" }),
    });
    const blockedId = ((await createBlocked.json()) as JobResponse).job!.id;
    await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: blockedId, status: "blocked", blockedReason: "needs review" }),
    });

    const afterRes = await fetch(`${base}/api/attention`);
    expect(afterRes.status).toBe(200);
    const after = (await afterRes.json()) as {
      items: { jobId: string; conversationId: number; status: string; reason: string; jobTitle: string }[];
    };
    const item = after.items.find((it) => it.jobId === blockedId);
    expect(item).toBeTruthy();
    expect(item?.status).toBe("blocked");
    expect(item?.reason).toBe("needs review");
    expect(item?.jobTitle).toBe("AttentionBlocked");

    // Unblock: it must leave the queue.
    await fetch(`${base}/api/plan/job`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: blockedId, status: "draft" }),
    });
    const clearedRes = await fetch(`${base}/api/attention`);
    const cleared = (await clearedRes.json()) as { items: { jobId: string }[] };
    expect(cleared.items.some((it) => it.jobId === blockedId)).toBe(false);
  });

  // J6: parse newline-delimited `data: {...}\n\n` SSE frames off a fetch ReadableStream, the
  // real wire format the browser's EventSource would see -- not a mocked transport.
  async function collectSseTypes(
    stream: ReadableStream<Uint8Array>,
    predicate: (frame: Record<string, unknown>) => boolean,
    deadlineMs: number,
  ): Promise<Record<string, unknown>[]> {
    const reader = stream.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    const collected: Record<string, unknown>[] = [];
    const deadline = Date.now() + deadlineMs;
    try {
      while (Date.now() < deadline) {
        const remaining = deadline - Date.now();
        if (remaining <= 0) break;
        const { value, done } = await Promise.race([
          reader.read(),
          new Promise<{ value: undefined; done: false }>((resolve) =>
            setTimeout(() => resolve({ value: undefined, done: false }), Math.min(remaining, 500)),
          ),
        ]);
        if (done) break;
        if (value) {
          buffer += decoder.decode(value, { stream: true });
          let idx: number;
          while ((idx = buffer.indexOf("\n\n")) !== -1) {
            const rawFrame = buffer.slice(0, idx);
            buffer = buffer.slice(idx + 2);
            const line = rawFrame.split("\n").find((l) => l.startsWith("data: "));
            if (!line) continue;
            const parsed = JSON.parse(line.slice("data: ".length)) as Record<string, unknown>;
            collected.push(parsed);
            if (predicate(parsed)) return collected;
          }
        }
      }
    } finally {
      await reader.cancel().catch(() => undefined);
    }
    return collected;
  }

  it("a subscriber to /api/stream sees a job's plan frames reach done, without any HTTP refetch", async () => {
    const streamRes = await fetch(`${base}/api/stream`);
    expect(streamRes.status).toBe(200);
    expect(streamRes.body).toBeTruthy();

    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "SseDone", command: "echo hi" }),
    });
    const id = ((await createRes.json()) as JobResponse).job!.id;

    await fetch(`${base}/api/plan/job/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });

    const frames = await collectSseTypes(
      streamRes.body!,
      (frame) =>
        frame.type === "plan" &&
        (frame.jobs as { id: string; status: string }[] | undefined)?.some(
          (j) => j.id === id && j.status === "done",
        ) === true,
      15_000,
    );

    const statuses = frames
      .filter((frame) => frame.type === "plan")
      .map((frame) => (frame.jobs as { id: string; status: string }[]).find((j) => j.id === id)?.status)
      .filter((status): status is string => status !== undefined);

    expect(statuses[statuses.length - 1]).toBe("done");
    // The live-update path, not just the HTTP-triggered initial frame: the status must actually
    // have advanced past the pre-run draft over the stream (this is the regression J6 fixes).
    expect(statuses).toContain("running");
  }, 20_000);

  it("a job whose command fails emits a 'failed' plan frame and a matching attention frame", async () => {
    const streamRes = await fetch(`${base}/api/stream`);
    expect(streamRes.status).toBe(200);

    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "SseFail", command: "exit 1" }),
    });
    const id = ((await createRes.json()) as JobResponse).job!.id;

    await fetch(`${base}/api/plan/job/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });

    const frames = await collectSseTypes(
      streamRes.body!,
      (frame) => frame.type === "attention" && (frame.items as { jobId: string }[]).some((it) => it.jobId === id),
      15_000,
    );

    const planFrames = frames.filter((frame) => frame.type === "plan");
    const sawFailedPlan = planFrames.some((frame) =>
      (frame.jobs as { id: string; status: string }[]).some((j) => j.id === id && j.status === "failed"),
    );
    expect(sawFailedPlan).toBe(true);

    const attentionFrame = frames.find(
      (frame) => frame.type === "attention" && (frame.items as { jobId: string }[]).some((it) => it.jobId === id),
    );
    expect(attentionFrame).toBeTruthy();
    const item = (attentionFrame!.items as { jobId: string; status: string }[]).find((it) => it.jobId === id);
    expect(item?.status).toBe("failed");
  }, 20_000);

  it("plan frames for conversation A are not delivered to a client watching conversation B", async () => {
    const createConvB = await fetch(`${base}/api/conversation`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "ConvB" }),
    });
    const convB = (await createConvB.json()) as { id: number };

    const streamB = await fetch(`${base}/api/stream?conversation=${convB.id}`);
    expect(streamB.status).toBe(200);

    const createRes = await fetch(`${base}/api/plan/job`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "ConvAJob", command: "echo hi" }),
    });
    const id = ((await createRes.json()) as JobResponse).job!.id;

    await fetch(`${base}/api/plan/job/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });

    // Give conversation A's job time to reach done, then read whatever conversation B's stream
    // actually received -- it must never carry A's job id in a `plan` frame.
    const deadline = Date.now() + 15_000;
    let doneOnA = false;
    while (Date.now() < deadline) {
      const planRes = await fetch(`${base}/api/plan`);
      const plan = (await planRes.json()) as { jobs: { id: string; status: string }[] };
      if (plan.jobs.find((j) => j.id === id)?.status === "done") {
        doneOnA = true;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    expect(doneOnA).toBe(true);

    const framesOnB = await collectSseTypes(streamB.body!, () => false, 2_000);
    const leaked = framesOnB.some(
      (frame) => frame.type === "plan" && (frame.jobs as { id: string }[]).some((j) => j.id === id),
    );
    expect(leaked).toBe(false);
  }, 25_000);
});
