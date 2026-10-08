import { describe, expect, it, beforeAll, afterAll } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// K1: only the root conversation ever pushed `messages` SSE frames; every child conversation
// was frozen after its initial snapshot. These tests drive the real daemon process (the same
// spawn pattern as boot.test.ts / planJobHttp.test.ts) and prove child conversations now get
// live `messages` frames, scoped per-conversation, with no cross-conversation leakage.

type ConversationCreateResponse = { id?: number; error?: string };
type MessagesFrame = { type: "messages"; messages: { id: number; role: string; content: string }[] };

describe("per-conversation messages SSE streaming (K1)", () => {
  let child: ChildProcess;
  let dir: string;
  let port: number;
  let base: string;

  beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-conversation-stream-"));
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

  async function createConversation(title: string): Promise<number> {
    const res = await fetch(`${base}/api/conversation`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title }),
    });
    const body = (await res.json()) as ConversationCreateResponse;
    return body.id!;
  }

  async function prompt(conversationId: number, text: string): Promise<void> {
    const res = await fetch(`${base}/api/prompt?conversation=${conversationId}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text }),
    });
    expect(res.status).toBe(200);
  }

  // Reads newline-delimited `data: {...}\n\n` SSE frames off a fetch ReadableStream reader (the
  // real wire format EventSource sees), same helper shape as planJobHttp.test.ts's
  // collectSseTypes, except it takes an already-acquired reader so a stream can be read from
  // across multiple calls (and explicitly cancelled later) without a "stream already locked"
  // error from calling getReader() twice.
  async function collectFrames(
    reader: ReadableStreamDefaultReader<Uint8Array>,
    predicate: (frame: Record<string, unknown>) => boolean,
    deadlineMs: number,
  ): Promise<Record<string, unknown>[]> {
    const decoder = new TextDecoder();
    let buffer = "";
    const collected: Record<string, unknown>[] = [];
    const deadline = Date.now() + deadlineMs;
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
    return collected;
  }

  it("a client watching a CHILD conversation receives a messages frame with the assistant's reply", async () => {
    const childId = await createConversation("K1 child");
    const streamRes = await fetch(`${base}/api/stream?conversation=${childId}`);
    expect(streamRes.status).toBe(200);

    await prompt(childId, "hello from K1 test");

    const reader = streamRes.body!.getReader();
    const frames = await collectFrames(
      reader,
      (frame) =>
        frame.type === "messages" &&
        (frame.messages as MessagesFrame["messages"]).some((m) => m.role === "assistant"),
      20_000,
    );
    await reader.cancel().catch(() => undefined);

    const last = frames.filter((f) => f.type === "messages").pop() as MessagesFrame | undefined;
    expect(last).toBeTruthy();
    expect(last!.messages.some((m) => m.role === "assistant")).toBe(true);
  }, 25_000);

  it("a client watching conversation A gets no messages frame when conversation B is prompted", async () => {
    const convA = await createConversation("K1 A");
    const convB = await createConversation("K1 B");

    const streamA = await fetch(`${base}/api/stream?conversation=${convA}`);
    expect(streamA.status).toBe(200);

    await prompt(convB, "prompt for B only");

    // Wait for B to actually settle (via its own stream), then check A never saw it.
    const streamB = await fetch(`${base}/api/stream?conversation=${convB}`);
    const readerB = streamB.body!.getReader();
    const framesB = await collectFrames(
      readerB,
      (frame) =>
        frame.type === "messages" &&
        (frame.messages as MessagesFrame["messages"]).some((m) => m.role === "assistant"),
      20_000,
    );
    await readerB.cancel().catch(() => undefined);
    expect(framesB.some((f) => f.type === "messages")).toBe(true);

    const readerA = streamA.body!.getReader();
    const framesA = await collectFrames(readerA, () => false, 2_000);
    await readerA.cancel().catch(() => undefined);
    const leaked = framesA.some(
      (frame) =>
        frame.type === "messages" &&
        (frame.messages as MessagesFrame["messages"]).some((m) => m.content === "prompt for B only"),
    );
    expect(leaked).toBe(false);
  }, 30_000);

  it("two clients watching the same child both get the update; one disconnecting doesn't kill the other's", async () => {
    const childId = await createConversation("K1 shared");

    const stream1 = await fetch(`${base}/api/stream?conversation=${childId}`);
    const stream2 = await fetch(`${base}/api/stream?conversation=${childId}`);
    expect(stream1.status).toBe(200);
    expect(stream2.status).toBe(200);
    const reader1 = stream1.body!.getReader();
    const reader2 = stream2.body!.getReader();

    await prompt(childId, "shared turn one");

    const frames1 = await collectFrames(
      reader1,
      (frame) =>
        frame.type === "messages" &&
        (frame.messages as MessagesFrame["messages"]).some((m) => m.role === "assistant"),
      20_000,
    );
    expect(frames1.some((f) => f.type === "messages")).toBe(true);

    // Disconnect client 1, then prompt again -- client 2 must still get the next update.
    await reader1.cancel().catch(() => undefined);

    await prompt(childId, "shared turn two");

    const frames2 = await collectFrames(
      reader2,
      (frame) =>
        frame.type === "messages" &&
        (frame.messages as MessagesFrame["messages"]).filter((m) => m.role === "assistant").length >= 2,
      20_000,
    );
    await reader2.cancel().catch(() => undefined);
    const last2 = frames2.filter((f) => f.type === "messages").pop() as MessagesFrame | undefined;
    expect(last2).toBeTruthy();
    // After both turns, at least 4 messages (2 user + 2 assistant) should be visible.
    expect(last2!.messages.length).toBeGreaterThanOrEqual(4);
  }, 30_000);

  it("after the last watcher of a conversation disconnects, a fresh watcher still gets live updates (no leaked/broken subscription)", async () => {
    const childId = await createConversation("K1 cleanup");

    const stream1 = await fetch(`${base}/api/stream?conversation=${childId}`);
    expect(stream1.status).toBe(200);
    const reader1 = stream1.body!.getReader();
    await reader1.cancel().catch(() => undefined);

    // Give the server a beat to process the disconnect and tear down its subscription.
    await new Promise((resolve) => setTimeout(resolve, 300));

    await prompt(childId, "after reconnect");

    const stream2 = await fetch(`${base}/api/stream?conversation=${childId}`);
    expect(stream2.status).toBe(200);
    const reader2 = stream2.body!.getReader();
    const frames2 = await collectFrames(
      reader2,
      (frame) =>
        frame.type === "messages" &&
        (frame.messages as MessagesFrame["messages"]).some((m) => m.role === "assistant"),
      20_000,
    );
    await reader2.cancel().catch(() => undefined);
    expect(frames2.some((f) => f.type === "messages")).toBe(true);
  }, 30_000);
});
