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
});
