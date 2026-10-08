import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach } from "vitest";
import { FileCredentialStore } from "./credentials.ts";
import { flattenMessages, sseFrame } from "./engine.ts";

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
