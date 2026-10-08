import { describe, expect, it, beforeAll, afterAll } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// HTTP-level tests for L4 (`POST /api/archive`) per the conversationHttp.test.ts /
// planJobHttp.test.ts pattern: start the real daemon process over an ephemeral port, drive it
// with `fetch`. Also verifies D (archiving must never mutate pi's own entries table) by reading
// the sqlite file directly.

type ConversationCreateResponse = { id?: number; error?: string };
type ConversationNode = { id: number; parentId: number | null; at: number | null; title: string; archived: boolean };
type ConversationsResponse = { conversations: ConversationNode[] };
type ArchiveResponse = { archived?: boolean; error?: string };

describe("archive HTTP route", () => {
  let child: ChildProcess;
  let dir: string;
  let port: number;
  let base: string;
  let dbPath: string;

  beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-archive-http-"));
    port = 21000 + Math.floor(Math.random() * 9000);
    dbPath = join(dir, "test.sqlite");
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

  async function createConversation(): Promise<number> {
    const res = await fetch(`${base}/api/conversation`, { method: "POST" });
    const body = (await res.json()) as ConversationCreateResponse;
    return body.id!;
  }

  async function listConversations(): Promise<ConversationNode[]> {
    const res = await fetch(`${base}/api/conversations`);
    const body = (await res.json()) as ConversationsResponse;
    return body.conversations;
  }

  it("POST archived:true hides the conversation from the default list", async () => {
    const id = await createConversation();

    const res = await fetch(`${base}/api/archive`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ conversation: id, archived: true }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as ArchiveResponse;
    expect(body.archived).toBe(true);

    const list = await listConversations();
    const found = list.find((c) => c.id === id);
    expect(found?.archived).toBe(true);
  });

  it("A: POST archived:false un-archives a previously archived conversation (idempotent both ways)", async () => {
    const id = await createConversation();

    await fetch(`${base}/api/archive`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ conversation: id, archived: true }),
    });
    const unarchiveRes = await fetch(`${base}/api/archive`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ conversation: id, archived: false }),
    });
    expect(unarchiveRes.status).toBe(200);
    const unarchiveBody = (await unarchiveRes.json()) as ArchiveResponse;
    expect(unarchiveBody.archived).toBe(false);

    const list = await listConversations();
    const found = list.find((c) => c.id === id);
    expect(found?.archived).toBe(false);
  });

  function countEntries(): number {
    // `node:sqlite` can't be ES-module-imported through Vite's transform pipeline (see
    // plans.test.ts's fakeDatabase comment) so shell out to a plain node process instead --
    // same escape hatch, applied to a read instead of a fake.
    const { execFileSync } = require("node:child_process") as typeof import("node:child_process");
    const out = execFileSync(
      process.execPath,
      [
        "-e",
        `const { DatabaseSync } = require("node:sqlite"); const db = new DatabaseSync(process.argv[1], { readOnly: true }); console.log(db.prepare("SELECT COUNT(*) as n FROM entries").get().n); db.close();`,
        dbPath,
      ],
      { encoding: "utf8" },
    );
    return Number(out.trim());
  }

  it("D: archiving does not change pi's own entries row count", async () => {
    const id = await createConversation();

    const beforeCount = countEntries();

    await fetch(`${base}/api/archive`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ conversation: id, archived: true }),
    });

    const afterCount = countEntries();

    expect(afterCount).toBe(beforeCount);
  });
});
