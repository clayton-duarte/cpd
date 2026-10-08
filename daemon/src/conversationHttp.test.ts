import { describe, expect, it, beforeAll, afterAll } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// HTTP-level tests for I1 (`POST /api/conversation`) per D121: start the real daemon process
// over an ephemeral port and drive it with `fetch`, the same pattern `planJobHttp.test.ts` uses.

type ConversationCreateResponse = { id?: number; error?: string };
type ConversationsResponse = {
  conversations: { id: number; parentId: number | null; at: number | null; title: string }[];
};
type PlanResponse = { jobs: unknown[]; error?: string };
type PromptResponse = { status?: string; reason?: string; error?: string };

describe("conversation-create HTTP route (I1)", () => {
  let child: ChildProcess;
  let dir: string;
  let port: number;
  let base: string;

  beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-conversation-http-"));
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

  it("POST with no body creates a root conversation visible in GET /api/conversations", async () => {
    const res = await fetch(`${base}/api/conversation`, { method: "POST" });
    expect(res.status).toBe(200);
    const body = (await res.json()) as ConversationCreateResponse;
    expect(typeof body.id).toBe("number");
    const newId = body.id!;

    const listRes = await fetch(`${base}/api/conversations`);
    const list = (await listRes.json()) as ConversationsResponse;
    const found = list.conversations.find((c) => c.id === newId);
    expect(found).toBeTruthy();
    expect(found?.parentId).toBeNull();
  });

  it("POST with a title uses that title in GET /api/conversations", async () => {
    const res = await fetch(`${base}/api/conversation`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: "My session" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as ConversationCreateResponse;
    const newId = body.id!;

    const listRes = await fetch(`${base}/api/conversations`);
    const list = (await listRes.json()) as ConversationsResponse;
    const found = list.conversations.find((c) => c.id === newId);
    expect(found?.title).toBe("My session");
  });

  it("POST with a non-string title rejects with 400", async () => {
    const res = await fetch(`${base}/api/conversation`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title: 123 }),
    });
    expect(res.status).toBe(400);
  });

  it("the new id accepts POST /api/prompt (not 404)", async () => {
    const createRes = await fetch(`${base}/api/conversation`, { method: "POST" });
    const created = (await createRes.json()) as ConversationCreateResponse;
    const newId = created.id!;

    const promptRes = await fetch(`${base}/api/prompt?conversation=${newId}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "hello" }),
    });
    expect(promptRes.status).not.toBe(404);
    const promptBody = (await promptRes.json()) as PromptResponse;
    expect(promptBody.error).toBeUndefined();
  }, 30_000);

  it("GET /api/plan for the new id returns 200 with an empty job list", async () => {
    const createRes = await fetch(`${base}/api/conversation`, { method: "POST" });
    const created = (await createRes.json()) as ConversationCreateResponse;
    const newId = created.id!;

    const planRes = await fetch(`${base}/api/plan?conversation=${newId}`);
    expect(planRes.status).toBe(200);
    const plan = (await planRes.json()) as PlanResponse;
    expect(plan.jobs).toEqual([]);
  });

  it("GET /api/plan for a never-created id still 404s", async () => {
    const res = await fetch(`${base}/api/plan?conversation=999999`);
    expect(res.status).toBe(404);
  });
});
