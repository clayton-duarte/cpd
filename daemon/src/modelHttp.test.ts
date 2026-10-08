import { describe, expect, it, beforeAll, afterAll } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// HTTP-level tests for L5 (model GET/PATCH + global default inheritance): start the real daemon
// process over an ephemeral port and drive it with `fetch`, the same pattern
// `conversationHttp.test.ts` uses. `CPD_DEFAULT_MODEL_PROVIDER`/`CPD_DEFAULT_MODEL_ID` are set to
// a distinguishable value so assertions never pass by coincidentally matching the real fallback.

type ConversationCreateResponse = { id?: number; error?: string };
type ModelRef = { provider: string; modelId: string };
type ModelResponse = { model?: ModelRef; error?: string };

const DEFAULT_MODEL: ModelRef = { provider: "test-provider", modelId: "test-model-default" };
const OTHER_MODEL: ModelRef = { provider: "test-provider", modelId: "test-model-other" };

describe("model HTTP routes (L5)", () => {
  let child: ChildProcess;
  let dir: string;
  let port: number;
  let base: string;

  beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-model-http-"));
    port = 21000 + Math.floor(Math.random() * 9000);
    const dbPath = join(dir, "test.sqlite");
    base = `http://localhost:${port}`;

    child = spawn(
      process.execPath,
      ["--experimental-strip-types", join(import.meta.dirname, "index.ts")],
      {
        env: {
          ...process.env,
          CPD_DAEMON_PORT: String(port),
          CPD_DB: dbPath,
          CPD_DEFAULT_MODEL_PROVIDER: DEFAULT_MODEL.provider,
          CPD_DEFAULT_MODEL_ID: DEFAULT_MODEL.modelId,
        },
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

  it("[B] a newly created conversation inherits the global default model, not a hardcoded one", async () => {
    const id = await createConversation();
    const res = await fetch(`${base}/api/model?conversation=${id}`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as ModelResponse;
    expect(body.model).toEqual(DEFAULT_MODEL);
  });

  it("[A] PATCH /api/model writes the named conversation, not the global default", async () => {
    const conversationA = await createConversation();
    const conversationB = await createConversation();

    const patchRes = await fetch(`${base}/api/model`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ conversation: conversationA, ...OTHER_MODEL }),
    });
    expect(patchRes.status).toBe(200);

    // A changed...
    const getA = await fetch(`${base}/api/model?conversation=${conversationA}`);
    const bodyA = (await getA.json()) as ModelResponse;
    expect(bodyA.model).toEqual(OTHER_MODEL);

    // ...but B (never patched) still resolves to the untouched global default, proving the
    // write landed on conversation A alone rather than mutating the shared default.
    const getB = await fetch(`${base}/api/model?conversation=${conversationB}`);
    const bodyB = (await getB.json()) as ModelResponse;
    expect(bodyB.model).toEqual(DEFAULT_MODEL);

    // A brand-new conversation created *after* the PATCH also still gets the real global
    // default, not conversationA's new model -- confirms the default itself was never rewritten.
    const conversationC = await createConversation();
    const getC = await fetch(`${base}/api/model?conversation=${conversationC}`);
    const bodyC = (await getC.json()) as ModelResponse;
    expect(bodyC.model).toEqual(DEFAULT_MODEL);
  });
});
