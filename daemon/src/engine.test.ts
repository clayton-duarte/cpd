import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach, vi } from "vitest";
import { FileCredentialStore } from "./credentials.ts";
import { flattenMessages, sseFrame, submitPrompt, abortRun, type Engine } from "./engine.ts";
import type { Conversation } from "@earendil-works/pi-durable";

describe("FileCredentialStore", () => {
  let dir: string;

  afterEach(() => {
    if (dir) rmSync(dir, { recursive: true, force: true });
  });

  it("read() returns the stored entry for a provider", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({
        "github-copilot": { type: "oauth", access: "tok", refresh: "ref", expires: 0 },
      }),
    );
    const store = new FileCredentialStore(authPath);

    const cred = await store.read("github-copilot");

    expect(cred).toEqual({ type: "oauth", access: "tok", refresh: "ref", expires: 0 });
  });

  it("read() returns undefined for a missing provider key", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(authPath, JSON.stringify({}));
    const store = new FileCredentialStore(authPath);

    const cred = await store.read("nonexistent");

    expect(cred).toBeUndefined();
  });

  it("modify() round-trips a write", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({ "github-copilot": { type: "oauth", access: "old", refresh: "r", expires: 0 } }),
    );
    const store = new FileCredentialStore(authPath);

    const updated = await store.modify("github-copilot", async (current) => ({
      ...(current as { type: "oauth"; access: string; refresh: string; expires: number }),
      access: "new",
    }));

    expect(updated).toEqual({ type: "oauth", access: "new", refresh: "r", expires: 0 });
    const reread = await store.read("github-copilot");
    expect(reread).toEqual({ type: "oauth", access: "new", refresh: "r", expires: 0 });
  });

  it("list() reports providerId and type without secrets", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({
        "github-copilot": { type: "oauth", access: "tok", refresh: "ref", expires: 0 },
        "other": { type: "api_key", key: "shh" },
      }),
    );
    const store = new FileCredentialStore(authPath);

    const list = await store.list();

    expect(list).toEqual(
      expect.arrayContaining([
        { providerId: "github-copilot", type: "oauth" },
        { providerId: "other", type: "api_key" },
      ]),
    );
  });

  it("delete() removes the key", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({ "github-copilot": { type: "oauth", access: "tok", refresh: "ref", expires: 0 } }),
    );
    const store = new FileCredentialStore(authPath);

    await store.delete("github-copilot");

    const reread = await store.read("github-copilot");
    expect(reread).toBeUndefined();
  });

  it("produces an actionable error when the auth file is missing", async () => {
    const missingPath = join(tmpdir(), "cpd-auth-does-not-exist", "auth.json");
    const store = new FileCredentialStore(missingPath);

    await expect(store.read("github-copilot")).rejects.toThrow(
      /No GitHub Copilot credentials found at .*\. Run `pi` and log in first\./,
    );
  });
});

function entry(id: number, model: unknown): { id: number; model: unknown } {
  return { id, model };
}

describe("flattenMessages", () => {
  it("joins text parts of an assistant/user message into a plain string", () => {
    expect(
      flattenMessages([
        entry(7, [{ role: "user", content: [{ type: "text", text: "a" }, { type: "text", text: "b" }] }]),
      ] as never),
    ).toEqual([{ id: 7, role: "user", content: "ab" }]);
  });

  it("ignores non-text parts", () => {
    expect(
      flattenMessages([
        entry(11, [
          {
            role: "assistant",
            content: [
              { type: "text", text: "hi" },
              { type: "toolCall", id: "1", name: "x", arguments: {} },
            ],
          },
        ]),
      ] as never),
    ).toEqual([{ id: 11, role: "assistant", content: "hi" }]);
  });

  it("drops empty system messages", () => {
    expect(
      flattenMessages([
        entry(10, [{ role: "system", content: "" }]),
        entry(10, [{ role: "system", content: [] }]),
        entry(7, [{ role: "user", content: "hello" }]),
      ] as never),
    ).toEqual([{ id: 7, role: "user", content: "hello" }]);
  });

  it("passes through a plain string system message with content", () => {
    expect(flattenMessages([entry(10, [{ role: "system", content: "be nice" }])] as never)).toEqual([
      { id: 10, role: "system", content: "be nice" },
    ]);
  });

  it("skips toolResult messages", () => {
    expect(
      flattenMessages([
        entry(12, [{ role: "toolResult", content: "ignored" }]),
        entry(15, [{ role: "assistant", content: "ok" }]),
      ] as never),
    ).toEqual([{ id: 15, role: "assistant", content: "ok" }]);
  });

  it("preserves sparse ids as-is (never computed from position)", () => {
    const out = flattenMessages([
      entry(7, [{ role: "user", content: "a" }]),
      entry(10, [{ role: "system", content: "sys" }]),
      entry(15, [{ role: "assistant", content: "b" }]),
    ] as never);
    expect(out.map((m) => m.id)).toEqual([7, 10, 15]);
  });

  it("one entry yielding two messages keeps the same id for both", () => {
    const out = flattenMessages([
      entry(12, [
        { role: "user", content: "first" },
        { role: "assistant", content: "second" },
      ]),
    ] as never);
    expect(out).toEqual([
      { id: 12, role: "user", content: "first" },
      { id: 12, role: "assistant", content: "second" },
    ]);
  });

  it("skips entries with no model array", () => {
    expect(flattenMessages([{ id: 20 }] as never)).toEqual([]);
  });
});

describe("sseFrame", () => {
  it("serializes a payload to exactly data: {...}\\n\\n", () => {
    const frame = sseFrame({ type: "messages", messages: [] });
    expect(frame).toBe(`data: ${JSON.stringify({ type: "messages", messages: [] })}\n\n`);
  });
});

/** Minimal fake `Conversation` for `submitPrompt`: only `.submit()` is exercised, and the
 * returned handle's `.wait()` resolves to whatever `settled` the test configures. Mocks the
 * engine per L1's instructions -- no real model call. */
function fakeConversation(settled: { status: "done" | "unanswered"; reason?: string; detail?: unknown }): Conversation {
  return {
    submit: async () => ({
      wait: async () => settled,
    }),
  } as unknown as Conversation;
}

describe("submitPrompt", () => {
  it("returns status only, no reason/detail, when the submission is done", async () => {
    const result = await submitPrompt(fakeConversation({ status: "done" }), "hi");
    expect(result).toEqual({ status: "done" });
  });

  it("returns reason and detail when the submission is unanswered", async () => {
    const result = await submitPrompt(
      fakeConversation({ status: "unanswered", reason: "model_error", detail: "the real provider text" }),
      "hi",
    );
    expect(result).toEqual({ status: "unanswered", reason: "model_error", detail: "the real provider text" });
  });

  it("drops a non-string detail rather than passing it through", async () => {
    const result = await submitPrompt(
      fakeConversation({ status: "unanswered", reason: "model_error", detail: { not: "a string" } }),
      "hi",
    );
    expect(result).toEqual({ status: "unanswered", reason: "model_error", detail: undefined });
  });

  // Falsification table (L1), row A: submitPrompt must not drop `detail` from its return value.
  it("[A] the detail actually reaches the caller, not just the reason code", async () => {
    const result = await submitPrompt(
      fakeConversation({ status: "unanswered", reason: "model_error", detail: "unique-marker-detail-42" }),
      "hi",
    );
    expect(result.detail).toBe("unique-marker-detail-42");
  });
});

describe("abortRun", () => {
  function fakeEngineWithConversation(conversation: { abort: () => Promise<void> } | undefined) {
    const root = { id: 1, abort: conversation?.abort } as unknown as Engine["root"];
    return {
      root,
      harness: {
        conversation: async (id: number) => (id === 1 ? root : conversation ? ({ ...conversation, id }) : undefined),
      },
    } as unknown as Engine;
  }

  it("calls conversation.abort() on the root conversation when no id is given", async () => {
    const abort = vi.fn().mockResolvedValue(undefined);
    const engine = fakeEngineWithConversation({ abort });

    await abortRun(engine, undefined);

    expect(abort).toHaveBeenCalledTimes(1);
  });

  it("calls conversation.abort() on the given conversation id, not root", async () => {
    const rootAbort = vi.fn().mockResolvedValue(undefined);
    const childAbort = vi.fn().mockResolvedValue(undefined);
    const engine = {
      root: { id: 1, abort: rootAbort },
      harness: {
        conversation: async (id: number) => (id === 7 ? { id: 7, abort: childAbort } : undefined),
      },
    } as unknown as Engine;

    await abortRun(engine, 7);

    expect(childAbort).toHaveBeenCalledTimes(1);
    expect(rootAbort).not.toHaveBeenCalled();
  });

  it("is a no-op when the conversation id is unknown -- does not throw", async () => {
    const engine = {
      root: { id: 1, abort: vi.fn() },
      harness: { conversation: async () => undefined },
    } as unknown as Engine;

    await expect(abortRun(engine, 999)).resolves.toBeUndefined();
  });
});
